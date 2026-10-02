pub fn Layout(title: &str, children: Element) -> Element {
    <html>
        <head><title>{title}</title></head>
        <body>
            {children}
        </body>
    </html>
}

pub fn Card(title: &str, children: Element) -> Element {
    <div class="card">
        <h2>{title}</h2>
        <div class="card-body">
            {children}
        </div>
    </div>
}

pub fn NoChildren() -> Element {
    <div>No children here</div>
}

pub fn Page() -> Element {
    <Layout title="My App">
        <h1>Welcome</h1>
        <Card title="First">
            <p>Card one content</p>
        </Card>
        <Card title="Second">
            <p>Card two content</p>
        </Card>
    </Layout>
}
