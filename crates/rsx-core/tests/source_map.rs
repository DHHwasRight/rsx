//! Mapping generated Rust back to the template that produced it.

use rsx_core::{generate_with_map, parse, Options, Registry, SourceMap};

const SOURCE: &str = "\
pub fn Card(title: &str) -> Element {
    <div class=\"card\">
        <h3>{title}</h3>
    </div>
}

pub fn Page(name: &str) -> Element {
    <main>
        <Card title={name} />
    </main>
}
";

fn generated() -> (String, SourceMap) {
    let file = parse(SOURCE).expect("parse failed");
    let mut registry = Registry::new();
    registry.add_file(&file);
    generate_with_map(&file, &registry, &Options::default()).expect("generate failed")
}

/// The generated line containing `needle`, 1-based.
fn line_of(code: &str, needle: &str) -> u32 {
    code.lines()
        .position(|l| l.contains(needle))
        .map(|i| i as u32 + 1)
        .unwrap_or_else(|| panic!("no generated line contains {needle:?}\n{code}"))
}

#[test]
fn maps_a_generated_line_back_to_the_node_that_produced_it() {
    let (code, map) = generated();
    let mapping = map.lookup(line_of(&code, "__w.text(&(title))")).unwrap();
    // `<h3>{title}</h3>` is line 3 of the source.
    assert_eq!(mapping.line, 3);
}

#[test]
fn maps_a_component_call_to_where_it_is_written_not_where_it_is_defined() {
    let (code, map) = generated();
    let mapping = map.lookup(line_of(&code, "Card { title: name }")).unwrap();
    assert_eq!(mapping.line, 9);
}

#[test]
fn reports_columns_one_based() {
    let (code, map) = generated();
    let mapping = map.lookup(line_of(&code, "Card { title: name }")).unwrap();
    // `<Card … />` is indented by eight spaces, so it starts at column 9.
    assert_eq!(mapping.col, 9);
}

#[test]
fn attributes_a_run_of_static_html_to_where_the_run_began() {
    let (code, map) = generated();
    let mapping = map.lookup(line_of(&code, "__w.raw(\"<main>\")")).unwrap();
    // `<main>` opens on line 8, not line 9 where the component call that
    // follows it forces the run to be flushed.
    assert_eq!(mapping.line, 8);
    assert_eq!(mapping.col, 5);
}

#[test]
fn keeps_mappings_in_increasing_order_of_generated_line() {
    let (_, map) = generated();
    let lines: Vec<u32> = map.mappings().iter().map(|m| m.generated_line).collect();
    let mut sorted = lines.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(lines, sorted);
}

#[test]
fn falls_back_to_the_nearest_preceding_mapping() {
    let (code, map) = generated();
    let last = *map.mappings().last().unwrap();
    assert_eq!(map.lookup(last.generated_line + 50), Some(last));
    assert!(map.lookup(code.lines().count() as u32).is_some());
}

#[test]
fn has_no_mapping_for_the_banner_which_belongs_to_no_node() {
    let (_, map) = generated();
    assert!(map.lookup(1).is_none());
}

#[test]
fn is_empty_for_a_file_with_no_components() {
    let file = parse("use std::fmt::Display;\n").unwrap();
    let (_, map) = generate_with_map(&file, &Registry::new(), &Options::default()).unwrap();
    assert!(map.is_empty());
}

#[test]
fn does_not_change_the_generated_code() {
    let file = parse(SOURCE).unwrap();
    let mut registry = Registry::new();
    registry.add_file(&file);
    let plain = rsx_core::generate(&file, &registry, &Options::default()).unwrap();
    let (mapped, _) = generated();
    assert_eq!(plain, mapped);
}

#[test]
fn every_mapping_points_at_a_line_that_exists() {
    let (_, map) = generated();
    let source_lines = SOURCE.lines().count() as u32;
    for mapping in map.mappings() {
        assert!(
            mapping.line >= 1 && mapping.line <= source_lines,
            "mapping {mapping:?} is outside the source"
        );
    }
}
