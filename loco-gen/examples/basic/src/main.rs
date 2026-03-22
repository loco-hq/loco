include!(concat!(env!("OUT_DIR"), "/loco_generated.rs"));

fn main() {
    // Load a specific instance by namespace
    let opportunity = Collection::load_instance("ben/crm.opportunity")
        .expect("should find opportunity instance");

    println!("Loaded instance: {:?}", opportunity);
    println!("  name: {}", opportunity.name());
    println!("  label: {}", opportunity.label());
    println!("  label_plural: {}", opportunity.label_plural());

    // List all instances
    println!("\nAll Collection instances:");
    for (namespace, instance) in Collection::load_all_instances() {
        println!("  {} -> {} ({})", namespace, instance.label(), instance.label_plural());
    }

    // Round-trip through cache
    let cache = loco_gen_runtime::TypedCache::new();
    opportunity.to_cache(&cache, "opp");

    let from_cache =
        Collection::from_cache(&cache, "opp").expect("should find collection in cache");

    assert_eq!(opportunity, from_cache);
    println!("\nCache round-trip successful! Objects are equal.");
}
