pub fn Card(title: &str, count: usize) -> Element {
    <div class="card">
        <h2>{title}</h2>
        <span>{count}</span>
    </div>
}

pub fn Badge(label: &str, active: bool) -> Element {
    <span class="badge" data-active?={active}>{label}</span>
}

pub fn Header(title: &str) -> Element {
    <header><h1>{title}</h1></header>
}

pub fn Footer() -> Element {
    <footer>Copyright 2026</footer>
}

pub fn Page(title: &str, items: &[String]) -> Element {
    <div class="page">
        <Card title={title} count={items.len()} />
        <Badge label="Status" active />
        <ul>
            for item in items {
                <li>{item}</li>
            }
        </ul>
    </div>
}

pub fn Layout(title: &str) -> Element {
    <html>
        <head><title>{title}</title></head>
        <body>
            <Header title={title} />
            <main>
                <Page title="Welcome" items={&["a".to_string(), "b".to_string()]} />
            </main>
            <Footer />
        </body>
    </html>
}
