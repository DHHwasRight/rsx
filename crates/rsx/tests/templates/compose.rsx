//! Composition: calls, children and spread.

use std::collections::BTreeMap;

pub fn Card(title: &str, children: Element) -> Element {
    <div class="card">
        <h2>{title}</h2>
        <div class="body">{children}</div>
    </div>
}

pub fn Badge(label: &str) -> Element {
    <span class="badge">{label}</span>
}

pub fn Layout(title: &str, children: Element) -> Element {
    <html>
        <head><title>{title}</title></head>
        <body>{children}</body>
    </html>
}

pub fn Page(name: &str) -> Element {
    <Layout title="Home">
        <Card title="First">
            <p>one</p>
            <Card title="Nested">
                <p>{name}</p>
            </Card>
        </Card>
        <Badge label="done" />
    </Layout>
}

pub fn Spread(attrs: &BTreeMap<String, String>) -> Element {
    <button class="btn" { ...attrs }>press</button>
}

pub fn Bools(attrs: &BTreeMap<String, bool>) -> Element {
    <input type="checkbox" { ...attrs }>
}

pub fn Generic<T: std::fmt::Display>(items: &[T]) -> Element {
    <ul>
        for item in items {
            <li>{item}</li>
        }
    </ul>
}

pub fn ChildlessCall() -> Element {
    <Card title="empty" />
}
