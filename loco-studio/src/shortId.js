// Base58 codec for compact UI IDs. UUIDs stay canonical in the DB and on the
// wire — we only encode for display and in-app URLs, then decode at the API
// boundary. Strings that don't look like base58 (e.g. a real UUID with dashes)
// pass through unchanged, so the codec is safe to apply twice.

const ALPHABET = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';
const BASE = 58n;
const SHORT_LEN = 22;
const UUID_HEX = /^[0-9a-f]{32}$/i;

export function encodeId(uuid) {
  if (!uuid) return uuid;
  const hex = uuid.replace(/-/g, '');
  if (!UUID_HEX.test(hex)) return uuid;
  let n = BigInt('0x' + hex);
  let out = '';
  while (n > 0n) {
    out = ALPHABET[Number(n % BASE)] + out;
    n = n / BASE;
  }
  return out.padStart(SHORT_LEN, ALPHABET[0]);
}

export function decodeId(short) {
  if (!short) return short;
  for (const c of short) {
    if (ALPHABET.indexOf(c) === -1) return short;
  }
  let n = 0n;
  for (const c of short) {
    n = n * BASE + BigInt(ALPHABET.indexOf(c));
  }
  const hex = n.toString(16).padStart(32, '0');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
