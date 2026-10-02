pub fn Page(logged_in: bool, name: &str, items: &[String]) -> Element {
    <div class="page">
        if logged_in {
            <p>Welcome, {name}!</p>
        } else {
            <p>Please log in.</p>
        }

        <ul>
            for item in items {
                <li>{item}</li>
            }
        </ul>
    </div>
}

pub fn IfOnly(show: bool) -> Element {
    if show {
        <p>Visible</p>
    }
}

pub fn Nested(logged_in: bool, items: &[String]) -> Element {
    <div>
        if logged_in {
            <ul>
                for item in items {
                    <li>{item}</li>
                }
            </ul>
        } else {
            <p>Please log in to see items.</p>
        }
    </div>
}

pub fn ComplexCondition(user: Option<&str>) -> Element {
    if let Some(user) = user {
        <span>Hello, {user}!</span>
    }
}

pub fn TopLevelLoop(names: &[String]) -> Element {
    for name in names {
        <p>{name}</p>
    }
}

pub fn MultipleBlocks(a: bool, b: bool) -> Element {
    if a {
        <p>A is true</p>
    }
    if b {
        <p>B is true</p>
    }
}

pub fn Indexed(items: &[String]) -> Element {
    <ol>
        for (index, item) in items.iter().enumerate() {
            <li>{index}. {item}</li>
        }
    </ol>
}

pub fn WhileLoop(mut n: u32) -> Element {
    <ul>
        while n > 0 {
            <li>{n}</li>
            n -= 1;
        }
    </ul>
}
