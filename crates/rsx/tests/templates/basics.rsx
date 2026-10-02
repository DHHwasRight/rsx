//! Elements, text, attributes and expressions.

pub fn Hello(name: &str) -> Element {
    <div class="greeting">
        <h1>Hello, {name}!</h1>
    </div>
}

pub fn Counts(count: usize, ratio: f64, flag: bool) -> Element {
    <ul>
        <li>{count}</li>
        <li>{ratio}</li>
        <li>{flag}</li>
    </ul>
}

pub fn Escaping(value: &str) -> Element {
    <p title={value}>{value}</p>
}

pub fn RawHtml(html: &str) -> Element {
    <div class="prose">{!html}</div>
}

pub fn Voids(src: &str) -> Element {
    <div>
        <img src={src} alt="a" />
        <br>
        <hr/>
        <input type="text" disabled>
    </div>
}

pub fn Conditional(active: bool, name: &str) -> Element {
    <input name={name} checked?={active} class="field">
}

pub fn Fragmented(a: &str, b: &str) -> Element {
    <>
        <span>{a}</span>
        <span>{b}</span>
    </>
}

pub fn Document() -> Element {
    <!DOCTYPE html>
    <html>
        <head>
            <!-- a comment -->
            <style>body { color: red; }</style>
            <script>if (a < b && c) { go("x"); }</script>
        </head>
        <body>done</body>
    </html>
}

pub fn Prose() -> Element {
    <p>
        if you ask me, text that starts with a keyword is still text
    </p>
}

pub fn Spacing(count: usize, name: &str) -> Element {
    <>
        <p>
            {count} items
        </p>
        <div><b>a</b> <i>b</i></div>
        <p>Hello, {name}!</p>
    </>
}
