//! A local BrickLink store API for developing without credentials (#135).
//!
//! ```text
//! cargo run -p loco-apps --example bricklink-mock [-- --port 3100 --seed 135 --date 2026-10-03]
//! ```
//!
//! Serves one generated day of a store's orders on the three calls the
//! `loco/bricklink` source makes (`src/bricklink/`):
//!
//! - `GET /orders?direction=in[&status=A,-B][&filed=true]`
//! - `GET /orders/{id}`
//! - `GET /orders/{id}/items`
//!
//! Point a dataset's `loco/bricklink.store:base_url` at it (`docs/brickos.md`).
//! It accepts any `Authorization: OAuth …` header and does not check the
//! signature: the signer is unit-tested in `src/bricklink/oauth.rs`, and the
//! `bricklink` Hurl suite checks it on the wire.
//!
//! Bodies are built from the fixture files the Hurl suite serves
//! (`tests/fixtures/bricklink/`): every order and line is a fixture object with
//! its values replaced, and replacing a key the fixture lacks panics. The two
//! cannot drift apart in shape.
//!
//! The day is seeded, so the same `--seed` and `--date` serve the same orders
//! on every run: 50 `PENDING` orders, a dozen further along, and three filed
//! ones (`GET /orders/{id}` answers them; the list leaves them out, as
//! BrickLink does without `filed=true`). Lines mix parts, sets, and minifigs,
//! from one lot to several hundred. A lot's `remarks` is its bin
//! (`A-12-3` for parts, `MF-4` for minifigs, `SET-2` for sets); about one lot in
//! twenty has a malformed one.
//! A big order sometimes comes in two batches, now and then with one lot on
//! a line in each, and a few lines have `inventory_id: null`. A path outside `/orders` is a bare HTTP 404, so a wrong
//! `base_url` fails the way it does against the fixture server.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::NaiveDate;
use serde_json::{json, Value};

const FIXTURE_ORDER: &str = include_str!("../tests/fixtures/bricklink/orders/29471234.json");
const FIXTURE_ITEMS: &str = include_str!("../tests/fixtures/bricklink/orders/29471234/items.json");
const FIXTURE_NOT_FOUND: &str = include_str!("../tests/fixtures/bricklink/errors/not_found.json");
const FIXTURE_BAD_OAUTH: &str = include_str!("../tests/fixtures/bricklink/errors/bad_oauth.json");

const DEFAULT_PORT: u16 = 3100;
const DEFAULT_SEED: u64 = 135;
const DEFAULT_DATE: &str = "2026-10-03";

const PENDING: usize = 50;
/// Unfiled orders past `PENDING`, by status.
const FURTHER: &[(&str, usize)] = &[("PAID", 6), ("PACKED", 4), ("SHIPPED", 2)];
/// Filed orders: reachable by id, not listed.
const FILED: usize = 3;

#[tokio::main]
async fn main() {
    let args = Args::parse(std::env::args().skip(1)).unwrap_or_else(|err| {
        eprintln!("{err}\n\nusage: bricklink-mock [--port N] [--seed N] [--date YYYY-MM-DD]");
        std::process::exit(2);
    });
    let day = Day::generate(args.seed, args.date);
    let pending = day
        .orders
        .iter()
        .filter(|order| order["status"] == "PENDING")
        .count();
    let lines: usize = day.items.values().map(|items| lines(items).len()).sum();
    let addr = format!("127.0.0.1:{}", args.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|err| panic!("failed to bind {addr}: {err}"));
    println!(
        "BrickLink mock on http://{addr}: {} orders ({pending} PENDING, {FILED} filed), {lines} lines, {} seed {}",
        day.orders.len(),
        args.date,
        args.seed,
    );
    axum::serve(listener, router(day)).await.unwrap();
}

struct Args {
    port: u16,
    seed: u64,
    date: NaiveDate,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut out = Args {
            port: DEFAULT_PORT,
            seed: DEFAULT_SEED,
            date: NaiveDate::parse_from_str(DEFAULT_DATE, "%Y-%m-%d").unwrap(),
        };
        while let Some(flag) = args.next() {
            let value = args.next().ok_or(format!("{flag} needs a value"))?;
            let bad = |_| format!("bad {flag}: {value}");
            match flag.as_str() {
                "--port" => out.port = value.parse().map_err(bad)?,
                "--seed" => out.seed = value.parse().map_err(bad)?,
                "--date" => {
                    out.date = NaiveDate::parse_from_str(&value, "%Y-%m-%d")
                        .map_err(|_| format!("bad --date: {value}"))?
                }
                _ => return Err(format!("unknown flag {flag}")),
            }
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

fn router(day: Day) -> Router {
    Router::new()
        .route("/orders", get(list_orders))
        .route("/orders/{id}", get(get_order))
        .route("/orders/{id}/items", get(get_items))
        .fallback(missing)
        .with_state(Arc::new(day))
}

/// Under `/orders`, BrickLink's own not-found envelope. Anywhere else, a bare
/// HTTP 404 with no envelope, as the fixture server answers a wrong `base_url`.
async fn missing(uri: Uri, headers: HeaderMap) -> Response {
    if uri.path().starts_with("/orders/") {
        return reply(&headers, uri.path(), None).into_response();
    }
    eprintln!("GET {uri} -> HTTP 404");
    (StatusCode::NOT_FOUND, "not found").into_response()
}

async fn list_orders(
    State(day): State<Arc<Day>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Json<Value> {
    // A store's purchases (`direction=out`) are none of this mock's business.
    let incoming = query.get("direction").is_none_or(|d| d == "in");
    let filed = query.get("filed").is_some_and(|f| f == "true");
    let status = StatusFilter::parse(query.get("status").map(String::as_str));
    let orders: Vec<Value> = day
        .orders
        .iter()
        .filter(|order| incoming && order["is_filed"] == filed)
        .filter(|order| status.admits(order["status"].as_str().unwrap_or_default()))
        .cloned()
        .collect();
    let what = format!("/orders?{}", query_text(&query));
    reply(&headers, &what, Some(Value::Array(orders)))
}

async fn get_order(
    State(day): State<Arc<Day>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Json<Value> {
    let wanted: Option<u64> = id.parse().ok();
    let order = day
        .orders
        .iter()
        .find(|order| wanted.is_some() && order["order_id"].as_u64() == wanted)
        .cloned();
    reply(&headers, &format!("/orders/{id}"), order)
}

async fn get_items(
    State(day): State<Arc<Day>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Json<Value> {
    let items = id
        .parse()
        .ok()
        .and_then(|id: u64| day.items.get(&id))
        .cloned();
    reply(&headers, &format!("/orders/{id}/items"), items)
}

/// The fixture envelope around `data`, or the fixture's not-found body. A
/// request with no OAuth header gets the fixture's `BAD_OAUTH_REQUEST`. All
/// HTTP 200, as BrickLink answers.
fn reply(headers: &HeaderMap, what: &str, data: Option<Value>) -> Json<Value> {
    let oauth = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("OAuth "));
    let body = if !oauth {
        let mut body = fixture(FIXTURE_BAD_OAUTH);
        body["meta"]["description"] = json!("no OAuth Authorization header");
        body
    } else if let Some(data) = data {
        let mut body = fixture(FIXTURE_ORDER);
        body["data"] = data;
        body
    } else {
        fixture(FIXTURE_NOT_FOUND)
    };
    let shown = match &body["data"] {
        // An order list is rows; an items reply is batches of lines.
        Value::Array(rows) => {
            let count: usize = rows.iter().map(|r| r.as_array().map_or(1, Vec::len)).sum();
            format!("{count} rows")
        }
        _ => body["meta"]["message"].as_str().unwrap_or_default().into(),
    };
    eprintln!("GET {what} -> {shown}");
    Json(body)
}

fn query_text(query: &HashMap<String, String>) -> String {
    let mut pairs: Vec<_> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();
    pairs.sort();
    pairs.join("&")
}

/// BrickLink's `status` parameter: comma-separated; a leading `-` excludes.
struct StatusFilter {
    include: Vec<String>,
    exclude: Vec<String>,
}

impl StatusFilter {
    fn parse(param: Option<&str>) -> Self {
        let (mut include, mut exclude) = (Vec::new(), Vec::new());
        for token in param.unwrap_or_default().split(',') {
            let token = token.trim().to_ascii_uppercase();
            match token.strip_prefix('-') {
                Some(rest) => exclude.push(rest.to_string()),
                None if !token.is_empty() => include.push(token),
                None => {}
            }
        }
        Self { include, exclude }
    }

    fn admits(&self, status: &str) -> bool {
        (self.include.is_empty() || self.include.iter().any(|s| s == status))
            && !self.exclude.iter().any(|s| s == status)
    }
}

// ---------------------------------------------------------------------------
// The generated day
// ---------------------------------------------------------------------------

/// Order details (also what the list returns), and each order's items as
/// BrickLink returns them: a list of batches, each a list of lines.
struct Day {
    orders: Vec<Value>,
    items: HashMap<u64, Value>,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Part,
    Set,
    Minifig,
}

struct CatalogItem {
    no: &'static str,
    name: &'static str,
    category_id: u32,
    kind: Kind,
}

const fn part(no: &'static str, name: &'static str, category_id: u32) -> CatalogItem {
    CatalogItem {
        no,
        name,
        category_id,
        kind: Kind::Part,
    }
}
const fn set(no: &'static str, name: &'static str, category_id: u32) -> CatalogItem {
    CatalogItem {
        no,
        name,
        category_id,
        kind: Kind::Set,
    }
}
const fn fig(no: &'static str, name: &'static str, category_id: u32) -> CatalogItem {
    CatalogItem {
        no,
        name,
        category_id,
        kind: Kind::Minifig,
    }
}

const CATALOG: &[CatalogItem] = &[
    part("3001", "Brick 2 x 4", 5),
    part("3003", "Brick 2 x 2", 5),
    part("3004", "Brick 1 x 2", 5),
    part("3005", "Brick 1 x 1", 5),
    part("3009", "Brick 1 x 6", 5),
    part("3010", "Brick 1 x 4", 5),
    part("3622", "Brick 1 x 3", 5),
    part("3020", "Plate 2 x 4", 26),
    part("3021", "Plate 2 x 3", 26),
    part("3022", "Plate 2 x 2", 26),
    part("3023", "Plate 1 x 2", 26),
    part("3024", "Plate 1 x 1", 26),
    part("3034", "Plate 2 x 8", 26),
    part("3666", "Plate 1 x 6", 26),
    part("3710", "Plate 1 x 4", 26),
    part("3795", "Plate 2 x 6", 26),
    part("3832", "Plate 2 x 10", 26),
    part("4073", "Plate, Round 1 x 1 Straight Side", 21),
    part(
        "3794b",
        "Plate, Modified 1 x 2 with 1 Stud with Groove (Jumper)",
        27,
    ),
    part("3069b", "Tile 1 x 2 with Groove", 37),
    part("3070b", "Tile 1 x 1 with Groove", 37),
    part("3068b", "Tile 2 x 2 with Groove", 37),
    part("2431", "Tile 1 x 4", 37),
    part("98138", "Tile, Round 1 x 1", 37),
    part("15712", "Tile, Modified 1 x 1 with Open O Clip", 15),
    part("3040", "Slope 45 2 x 1", 31),
    part("3039", "Slope 45 2 x 2", 31),
    part("54200", "Slope 30 1 x 1 x 2/3", 31),
    part("85984", "Slope 30 1 x 2 x 2/3", 31),
    part("3665", "Slope, Inverted 45 2 x 1", 31),
    part("3062b", "Brick, Round 1 x 1 Open Stud", 20),
    part("4070", "Brick, Modified 1 x 1 with Headlight", 6),
    part("87087", "Brick, Modified 1 x 1 with Stud on 1 Side", 6),
    part("30414", "Brick, Modified 1 x 4 with Studs on 1 Side", 6),
    part("60581", "Panel 1 x 4 x 3 with Side Supports", 23),
    part("2780", "Technic, Pin with Friction Ridges", 139),
    part("6558", "Technic, Pin 3L with Friction Ridges", 139),
    part("4274", "Technic, Pin 1/2", 139),
    part("32062", "Technic, Axle 2L Notched", 46),
    set("6086-1", "Black Knight's Castle", 186),
    set("6990-1", "Futuron Monorail Transport System", 128),
    set("10305-1", "Lion Knights' Castle", 186),
    set("10497-1", "Galaxy Explorer", 128),
    set("21318-1", "Tree House", 497),
    set("31120-1", "Medieval Castle", 672),
    set("40516-1", "Everyone Is Awesome", 535),
    set("42115-1", "Lamborghini Sian FKP 37", 12),
    set("60337-1", "Express Passenger Train", 52),
    set("71741-1", "NINJAGO City Gardens", 435),
    fig("sw0001a", "Luke Skywalker (Tatooine)", 65),
    fig("sw0578", "Clone Trooper, Phase 2", 65),
    fig("cas056", "Black Knight", 186),
    fig("cas093", "Crusader Lion, Gold Lion Shield", 186),
    fig("hp150", "Harry Potter, Gryffindor Robe", 230),
    fig("hp137", "Hermione Granger, Gryffindor Robe", 230),
    fig("njo0001", "Kai", 435),
    fig("col001", "Zombie, Series 1", 556),
    fig("cty0001", "Police Officer", 52),
    fig("sp003", "Futuron, Blue", 128),
    fig("pi003", "Pirate, Striped Shirt", 148),
    fig("sh0001", "Batman, Black Suit", 768),
];

/// BrickLink color ids.
const COLORS: &[(u32, &str)] = &[
    (1, "White"),
    (11, "Black"),
    (5, "Red"),
    (7, "Blue"),
    (3, "Yellow"),
    (86, "Light Bluish Gray"),
    (85, "Dark Bluish Gray"),
    (6, "Green"),
    (88, "Reddish Brown"),
    (2, "Tan"),
    (69, "Dark Tan"),
    (4, "Orange"),
    (12, "Trans-Clear"),
    (63, "Dark Blue"),
    (59, "Dark Red"),
];

/// Remarks a hurried human typed. #106 has to cope with every one.
const MALFORMED_REMARKS: &[&str] = &[
    "",
    "a-12-3",
    "A12-3",
    "A-12",
    "B--7",
    "C-1-2-3",
    "see blue drawer",
    "A-12-3 / overflow D-01-1",
    "MF4",
    " B-04-2",
];

const BUYERS: &[&str] = &[
    "ada",
    "grace",
    "linus",
    "marie",
    "alan",
    "hedy",
    "ken",
    "dennis",
    "barbara",
    "edsger",
    "margaret",
    "radia",
    "frances",
    "katherine",
    "donald",
    "niklaus",
    "sophie",
    "joan",
];
const BUYER_SUFFIXES: &[&str] = &["_bricks", "_mocs", "_lugs", "builds", "_plates", "_afol"];

/// One lot in the store's inventory. Orders draw lines from these, so a bin
/// recurs across orders as it does in a real store.
struct Lot {
    inventory_id: u64,
    item: &'static CatalogItem,
    color: (u32, &'static str),
    new_or_used: &'static str,
    completeness: &'static str,
    unit_price: f64,
    weight: f64,
    remarks: String,
}

impl Day {
    fn generate(seed: u64, date: NaiveDate) -> Day {
        let mut rng = Rng::new(seed);
        let lots = inventory(&mut rng);
        let item_template = fixture(FIXTURE_ITEMS)["data"][0][0].clone();
        let order_template = fixture(FIXTURE_ORDER)["data"].clone();

        let mut statuses: Vec<&str> = vec!["PENDING"; PENDING];
        for (status, count) in FURTHER {
            statuses.extend(std::iter::repeat_n(*status, *count));
        }
        statuses.extend(std::iter::repeat_n("COMPLETED", FILED));
        rng.shuffle(&mut statuses);

        let mut seconds: Vec<u32> = statuses.iter().map(|_| rng.below(86_400) as u32).collect();
        seconds.sort_unstable();

        let mut orders = Vec::new();
        let mut items = HashMap::new();
        let mut order_id = 29_480_000 + rng.below(1000);
        for (status, second) in statuses.into_iter().zip(seconds) {
            order_id += 1 + rng.below(40);
            let batches = order_lines(&mut rng, &lots, &item_template);
            let all: Vec<&Value> = batches.iter().flatten().collect();
            let total_count: u64 = all.iter().map(|l| l["quantity"].as_u64().unwrap()).sum();
            let subtotal: f64 = all.iter().map(|l| line_total(l)).sum();
            let weight: f64 = all
                .iter()
                .map(|l| l["quantity"].as_f64().unwrap() * parse(&l["weight"]))
                .sum();
            let shipping = if rng.chance(0.2) {
                0.0
            } else {
                3.5 + rng.below(900) as f64 / 100.0
            };

            let when = date
                .and_hms_opt(second / 3600, second / 60 % 60, second % 60)
                .unwrap()
                .format("%Y-%m-%dT%H:%M:%S.000Z")
                .to_string();
            let buyer = format!("{}{}", rng.pick(BUYERS), rng.pick(BUYER_SUFFIXES));

            let mut order = order_template.clone();
            put(&mut order, "order_id", json!(order_id));
            put(&mut order, "date_ordered", json!(when));
            put(&mut order, "date_status_changed", json!(when));
            put(&mut order, "buyer_name", json!(buyer));
            put(
                &mut order,
                "buyer_email",
                json!(format!("{buyer}@example.test")),
            );
            put(&mut order, "status", json!(status));
            put(&mut order, "is_filed", json!(status == "COMPLETED"));
            put(&mut order, "total_count", json!(total_count));
            put(&mut order, "unique_count", json!(all.len()));
            put(&mut order, "total_weight", json!(format!("{weight:.2}")));
            let payment = &mut order["payment"];
            put(payment, "date_paid", json!(when));
            let cost = &mut order["cost"];
            put(cost, "subtotal", json!(money(subtotal)));
            put(cost, "shipping", json!(money(shipping)));
            put(cost, "grand_total", json!(money(subtotal + shipping)));

            items.insert(order_id, json!(batches));
            orders.push(order);
        }
        Day { orders, items }
    }
}

/// The store's lots: every part in several colors and conditions, each set
/// and minifig once or twice.
fn inventory(rng: &mut Rng) -> Vec<Lot> {
    let mut lots = Vec::new();
    let mut inventory_id = 358_100_000 + rng.below(10_000);
    let (mut set_bin, mut fig_bin) = (0, 0);
    for item in CATALOG {
        let copies = match item.kind {
            Kind::Part => 4 + rng.below(6),
            Kind::Set | Kind::Minifig => 1 + rng.below(2),
        };
        let mut colors_used = HashSet::new();
        for _ in 0..copies {
            let color = match item.kind {
                Kind::Part => *rng.pick(COLORS),
                Kind::Set | Kind::Minifig => (0, "(Not Applicable)"),
            };
            let new_or_used = if rng.chance(0.7) { "N" } else { "U" };
            if !colors_used.insert((color.0, new_or_used)) {
                continue;
            }
            inventory_id += 1 + rng.below(500);
            let (unit_price, weight, completeness, bin) = match item.kind {
                Kind::Part => (
                    0.02 + rng.below(60) as f64 / 100.0,
                    0.1 + rng.below(400) as f64 / 100.0,
                    "X",
                    format!(
                        "{}-{:02}-{}",
                        rng.pick(&["A", "B", "C", "D", "E", "F"]),
                        1 + rng.below(24),
                        1 + rng.below(9)
                    ),
                ),
                Kind::Set => {
                    set_bin += 1;
                    (
                        25.0 + rng.below(40_000) as f64 / 100.0,
                        300.0 + rng.below(3000) as f64,
                        *rng.pick(&["C", "B", "S"]),
                        format!("SET-{set_bin}"),
                    )
                }
                Kind::Minifig => {
                    fig_bin += 1;
                    (
                        2.0 + rng.below(3800) as f64 / 100.0,
                        3.0 + rng.below(300) as f64 / 100.0,
                        "X",
                        format!("MF-{fig_bin}"),
                    )
                }
            };
            let remarks = if rng.chance(0.05) {
                rng.pick(MALFORMED_REMARKS).to_string()
            } else {
                bin
            };
            lots.push(Lot {
                inventory_id,
                item,
                color,
                new_or_used,
                completeness,
                unit_price,
                weight,
                remarks,
            });
        }
    }
    lots
}

/// One order's lines, in batches. Most orders are parts, some with a set or a
/// few minifigs; a big order sometimes arrives as two batches.
fn order_lines(rng: &mut Rng, lots: &[Lot], template: &Value) -> Vec<Vec<Value>> {
    let parts: Vec<&Lot> = lots.iter().filter(|l| l.item.kind == Kind::Part).collect();
    let sets: Vec<&Lot> = lots.iter().filter(|l| l.item.kind == Kind::Set).collect();
    let figs: Vec<&Lot> = lots
        .iter()
        .filter(|l| l.item.kind == Kind::Minifig)
        .collect();

    let mut chosen: Vec<&Lot> = Vec::new();
    let roll = rng.below(100);
    if roll < 12 {
        take(rng, &mut chosen, &sets, 1);
    }
    if (12..30).contains(&roll) {
        let n = 1 + rng.below(4);
        take(rng, &mut chosen, &figs, n);
    }
    if roll >= 8 {
        // Mostly a handful of lots; now and then a long list.
        let n = if rng.chance(0.15) {
            15 + rng.below(30)
        } else {
            1 + rng.below(8)
        };
        take(rng, &mut chosen, &parts, n);
    }

    let lines: Vec<Value> = chosen
        .into_iter()
        .map(|lot| {
            let quantity = match lot.item.kind {
                Kind::Set => 1 + u64::from(rng.chance(0.1)),
                Kind::Minifig => 1 + rng.below(3),
                Kind::Part => match rng.below(100) {
                    0..=39 => 1 + rng.below(10),
                    40..=69 => 11 + rng.below(40),
                    70..=89 => 51 + rng.below(150),
                    _ => 201 + rng.below(400),
                },
            };
            line(template, lot, quantity)
        })
        .collect();
    let mut batches = if lines.len() > 12 && rng.chance(0.4) {
        let split = lines.len() / 2;
        let (a, b) = lines.split_at(split);
        let (a, mut b) = (a.to_vec(), b.to_vec());
        // A lot already in batch 1 is sometimes bought again in batch 2, as
        // in the fixture: the same inventory id on two lines.
        if rng.chance(0.5) {
            let mut again = rng.pick(&a).clone();
            again["quantity"] = json!(1 + rng.below(20));
            b.push(again);
        }
        vec![a, b]
    } else {
        vec![lines]
    };
    // Now and then a line comes back with no inventory id, as the fixture's
    // last line does. The source names it by position (`{order}-{batch}-x{n}`).
    if rng.chance(0.06) {
        let last = batches.last_mut().and_then(|batch| batch.last_mut());
        if let Some(line) = last {
            line["inventory_id"] = Value::Null;
        }
    }
    batches
}

/// `n` draws from `pool`; a lot drawn twice is one line.
fn take<'a>(rng: &mut Rng, chosen: &mut Vec<&'a Lot>, pool: &[&'a Lot], n: u64) {
    for _ in 0..n {
        let lot = *rng.pick(pool);
        if !chosen.iter().any(|c| c.inventory_id == lot.inventory_id) {
            chosen.push(lot);
        }
    }
}

fn line(template: &Value, lot: &Lot, quantity: u64) -> Value {
    let price = json!(money(lot.unit_price));
    let mut line = template.clone();
    put(&mut line, "inventory_id", json!(lot.inventory_id));
    let item = &mut line["item"];
    put(item, "no", json!(lot.item.no));
    put(item, "name", json!(lot.item.name));
    put(
        item,
        "type",
        json!(match lot.item.kind {
            Kind::Part => "PART",
            Kind::Set => "SET",
            Kind::Minifig => "MINIFIG",
        }),
    );
    put(item, "category_id", json!(lot.item.category_id));
    put(&mut line, "color_id", json!(lot.color.0));
    put(&mut line, "color_name", json!(lot.color.1));
    put(&mut line, "quantity", json!(quantity));
    put(&mut line, "new_or_used", json!(lot.new_or_used));
    put(&mut line, "completeness", json!(lot.completeness));
    for key in [
        "unit_price",
        "unit_price_final",
        "disp_unit_price",
        "disp_unit_price_final",
    ] {
        put(&mut line, key, price.clone());
    }
    put(&mut line, "remarks", json!(lot.remarks));
    put(&mut line, "weight", json!(format!("{:.2}", lot.weight)));
    line
}

/// Replace a value the fixture has. A key it lacks is a drift from the shape
/// the source is tested against, so it fails loudly.
fn put(object: &mut Value, key: &str, value: Value) {
    match object.get_mut(key) {
        Some(slot) => *slot = value,
        None => panic!("the BrickLink fixture has no `{key}`; update tests/fixtures/bricklink/"),
    }
}

fn fixture(text: &str) -> Value {
    serde_json::from_str(text).expect("BrickLink fixture is JSON")
}

fn money(amount: f64) -> String {
    format!("{amount:.4}")
}

fn parse(value: &Value) -> f64 {
    value.as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0)
}

fn line_total(line: &Value) -> f64 {
    line["quantity"].as_f64().unwrap() * parse(&line["unit_price_final"])
}

/// Lines of one order's items, flattened across batches.
fn lines(items: &Value) -> Vec<&Value> {
    items
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|batch| batch.as_array().into_iter().flatten())
        .collect()
}

/// SplitMix64: small, seeded, and the same on every platform.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// `0..n`. The modulo bias is irrelevant at these sizes.
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn chance(&mut self, p: f64) -> bool {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64 <= p
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            items.swap(i, self.below(i as u64 + 1) as usize);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day() -> Day {
        Day::generate(DEFAULT_SEED, NaiveDate::from_ymd_opt(2026, 10, 3).unwrap())
    }

    fn all_lines(day: &Day) -> Vec<&Value> {
        day.items.values().flat_map(lines).collect()
    }

    fn keys(value: &Value) -> Vec<String> {
        let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    }

    #[test]
    fn a_seed_serves_the_same_day_every_run() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
        let (a, b) = (Day::generate(7, date), Day::generate(7, date));
        assert_eq!(a.orders, b.orders);
        assert_eq!(a.items, b.items);
        assert_ne!(a.orders, Day::generate(8, date).orders);
    }

    #[test]
    fn orders_and_lines_have_exactly_the_fixture_shape() {
        let day = day();
        let order = &fixture(FIXTURE_ORDER)["data"];
        let line = &fixture(FIXTURE_ITEMS)["data"][0][0];
        for generated in &day.orders {
            assert_eq!(keys(generated), keys(order));
            assert_eq!(keys(&generated["cost"]), keys(&order["cost"]));
        }
        for generated in all_lines(&day) {
            assert_eq!(keys(generated), keys(line));
            assert_eq!(keys(&generated["item"]), keys(&line["item"]));
            assert!(generated["quantity"].is_u64());
            assert!(generated["color_id"].is_u64());
        }
    }

    #[test]
    fn a_realistic_day() {
        let day = day();
        let unfiled: Vec<_> = day
            .orders
            .iter()
            .filter(|o| o["is_filed"] == false)
            .collect();
        let pending = unfiled.iter().filter(|o| o["status"] == "PENDING").count();
        assert_eq!(pending, PENDING);
        assert_eq!(day.orders.len() - unfiled.len(), FILED);
        for order in &day.orders {
            let id = order["order_id"].as_u64().unwrap();
            assert!(!lines(&day.items[&id]).is_empty(), "order {id} has lines");
        }

        let lines = all_lines(&day);
        for kind in ["PART", "SET", "MINIFIG"] {
            assert!(lines.iter().any(|l| l["item"]["type"] == kind), "{kind}");
        }
        let qty: Vec<u64> = lines
            .iter()
            .map(|l| l["quantity"].as_u64().unwrap())
            .collect();
        assert_eq!(qty.iter().min(), Some(&1));
        assert!(qty.iter().filter(|q| **q >= 200).count() >= 5, "scale lots");
        assert!(
            qty.iter().filter(|q| **q <= 10).count() >= 50,
            "hand counts"
        );

        let malformed = lines
            .iter()
            .filter(|l| MALFORMED_REMARKS.contains(&l["remarks"].as_str().unwrap()))
            .count();
        assert!(malformed >= 3, "only {malformed} malformed remarks");
        assert!(malformed * 5 < lines.len(), "malformed remarks are a few");

        let repeated = day.items.values().any(|items| {
            let batches = items.as_array().unwrap();
            let ids = |b: &Value| -> HashSet<u64> {
                b.as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|l| l["inventory_id"].as_u64())
                    .collect()
            };
            batches.len() == 2 && !ids(&batches[0]).is_disjoint(&ids(&batches[1]))
        });
        assert!(repeated, "some lot is in two batches of one order");

        let unnamed = lines.iter().filter(|l| l["inventory_id"].is_null()).count();
        assert!(unnamed >= 1, "some line has no inventory id");
        assert!(
            unnamed * 20 < lines.len(),
            "lines without an inventory id are rare"
        );
    }

    #[test]
    fn status_filter_follows_bricklink() {
        let f = StatusFilter::parse(Some("pending,PAID"));
        assert!(f.admits("PENDING") && f.admits("PAID") && !f.admits("PACKED"));
        let f = StatusFilter::parse(Some("-PENDING"));
        assert!(!f.admits("PENDING") && f.admits("PAID"));
        assert!(StatusFilter::parse(None).admits("SHIPPED"));
    }

    #[tokio::test]
    async fn serves_the_three_calls_the_source_makes() {
        let day = day();
        let first = day
            .orders
            .iter()
            .find(|o| o["status"] == "PENDING")
            .unwrap();
        let first_id = first["order_id"].as_u64().unwrap();
        let filed = day.orders.iter().find(|o| o["is_filed"] == true).unwrap();
        let filed_id = filed["order_id"].as_u64().unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router(day)).await });
        let client = reqwest::Client::new();
        let call = |path: String, oauth: bool| {
            let mut request = client.get(format!("{base}{path}"));
            if oauth {
                request = request.header("authorization", "OAuth oauth_consumer_key=\"x\"");
            }
            async move {
                let response = request.send().await.unwrap();
                assert_eq!(response.status(), 200);
                serde_json::from_str::<Value>(&response.text().await.unwrap()).unwrap()
            }
        };

        let pending = call("/orders?direction=in&status=PENDING".into(), true).await;
        assert_eq!(pending["meta"]["code"], 200);
        assert_eq!(pending["data"].as_array().unwrap().len(), PENDING);
        let unfiled = call("/orders?direction=in".into(), true).await;
        let count = unfiled["data"].as_array().unwrap().len();
        assert_eq!(count, PENDING + FURTHER.iter().map(|f| f.1).sum::<usize>());
        let ids: Vec<_> = unfiled["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o["order_id"].as_u64().unwrap())
            .collect();
        assert!(!ids.contains(&filed_id));

        let order = call(format!("/orders/{first_id}"), true).await;
        assert_eq!(order["data"]["order_id"], first_id);
        let order = call(format!("/orders/{filed_id}"), true).await;
        assert_eq!(order["data"]["is_filed"], true);
        let items = call(format!("/orders/{first_id}/items"), true).await;
        assert!(!lines(&items["data"]).is_empty());

        let missing = call("/orders/1/items".into(), true).await;
        assert_eq!(missing["meta"]["code"], 404);
        let unknown = client.get(format!("{base}/api/store/v1/orders")).send();
        assert_eq!(unknown.await.unwrap().status(), 404, "a wrong base_url");
        let refused = call("/orders?direction=in".into(), false).await;
        assert_eq!(refused["meta"]["message"], "BAD_OAUTH_REQUEST");
    }
}
