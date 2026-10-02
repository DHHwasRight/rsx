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

// --------------------------------------------------- byte-exact fragments

/// Compiles `source` and returns the generated code with its map.
fn compile(source: &str) -> (String, SourceMap) {
    let file = parse(source).expect("parse failed");
    let mut registry = Registry::new();
    registry.add_file(&file);
    generate_with_map(&file, &registry, &Options::default()).expect("generate failed")
}

/// The byte offset of `needle` in `haystack`, or a useful panic.
fn offset_of(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} not found in:\n{haystack}"))
}

#[test]
fn maps_an_interpolated_expression_byte_for_byte() {
    let source = "pub fn A(user: &User) -> Element { <p>{user.name}</p> }";
    let (code, map) = compile(source);

    let src_start = offset_of(source, "user.name");
    let gen_start = map.to_generated(src_start).expect("no mapping");
    assert!(
        code[gen_start..].starts_with("user.name"),
        "landed on {:?}",
        &code[gen_start..gen_start + 20]
    );

    // A cursor just after the dot maps to just after the dot.
    let after_dot = src_start + "user.".len();
    let gen_after = map.to_generated(after_dot).unwrap();
    assert!(code[gen_after..].starts_with("name"));
}

#[test]
fn maps_back_from_the_generated_code() {
    let source = "pub fn A(user: &User) -> Element { <p>{user.name}</p> }";
    let (code, map) = compile(source);

    let gen = offset_of(&code, "user.name");
    assert_eq!(map.to_source(gen), Some(offset_of(source, "user.name")));
}

#[test]
fn a_position_at_the_end_of_an_expression_still_maps() {
    // This is where the cursor sits after typing a `.`, which is exactly when
    // completion is asked for.
    let source = "pub fn A(user: &User) -> Element { <p>{user.}</p> }";
    let (code, map) = compile(source);

    let end = offset_of(source, "user.") + "user.".len();
    let gen = map.to_generated(end).expect("no mapping at the end");
    assert!(
        code[..gen].ends_with("user."),
        "landed after {:?}",
        &code[..gen]
    );
}

#[test]
fn maps_every_kind_of_rust_fragment() {
    let source = "\
pub fn A(user: &User, items: &[Row], extra: &Attrs, flag: bool) -> Element {
    let greeting = user.greeting();
    <div class={user.class_name()} hidden?={user.hidden} { ...extra.all() }>
        {greeting}
        {!user.raw_bio}
        if user.verified {
            <b>yes</b>
        } else if user.pending {
            <i>wait</i>
        }
        for row in items.iter() {
            <li>{row.title}</li>
        }
        match user.role() {
            Role::Admin => <b>admin</b>,
            _ => <span>member</span>,
        }
        <Card title={user.name} />
    </div>
}";
    let (code, map) = compile(source);

    for fragment in [
        "let greeting = user.greeting();",
        "user.class_name()",
        "user.hidden",
        "extra.all()",
        "&[Row]",
        "greeting",
        "user.raw_bio",
        "user.verified",
        "user.pending",
        "row in items.iter()",
        "row.title",
        "user.role()",
        "Role::Admin",
        "user.name",
    ] {
        let src = offset_of(source, fragment);
        let gen = map
            .to_generated(src)
            .unwrap_or_else(|| panic!("no mapping for {fragment:?}"));
        assert!(
            code[gen..].starts_with(fragment),
            "{fragment:?} mapped to {:?}",
            &code[gen..(gen + fragment.len() + 10).min(code.len())]
        );
        assert_eq!(map.to_source(gen), Some(src), "round trip for {fragment:?}");
    }
}

#[test]
fn markup_has_no_rust_mapping() {
    let source = "pub fn A() -> Element { <div class=\"card\">text</div> }";
    let (_, map) = compile(source);

    assert!(!map.is_rust(offset_of(source, "card")));
    assert!(!map.is_rust(offset_of(source, "text")));
    assert!(!map.is_rust(offset_of(source, "<div")));
}

#[test]
fn fragments_do_not_overlap_and_stay_inside_both_texts() {
    let source = "\
pub fn A(user: &User, items: &[Row]) -> Element {
    <ul>
        for row in items {
            <li title={row.title}>{row.body}</li>
        }
    </ul>
}";
    let (code, map) = compile(source);

    let mut previous_end = 0usize;
    for fragment in map.fragments() {
        assert!(
            fragment.generated.start >= previous_end,
            "fragments overlap: {fragment:?}"
        );
        previous_end = fragment.generated.end;

        assert!(fragment.generated.end <= code.len());
        assert!(fragment.source.end <= source.len());
        // The two sides hold the same text, which is the whole premise.
        assert_eq!(
            &code[fragment.generated.clone()],
            &source[fragment.source.clone()],
            "fragment contents differ"
        );
    }
    assert!(!map.fragments().is_empty());
}
