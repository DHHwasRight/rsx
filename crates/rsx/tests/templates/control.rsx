//! Rust control flow inside markup.

pub enum Status {
    Ok,
    Pending,
    Failed(u32),
}

pub fn Branches(role: &str) -> Element {
    if role == "admin" {
        <b>Admin</b>
    } else if role == "mod" {
        <i>Moderator</i>
    } else {
        <span>Member</span>
    }
}

pub fn NoElse(show: bool) -> Element {
    <div>
        if show {
            <p>visible</p>
        }
    </div>
}

pub fn Loop(items: &[String]) -> Element {
    <ul>
        for item in items {
            <li>{item}</li>
        }
    </ul>
}

pub fn Indexed(items: &[String]) -> Element {
    <ol>
        for (i, item) in items.iter().enumerate() {
            <li data-index={i}>{item}</li>
        }
    </ol>
}

pub fn Matched(status: &Status) -> Element {
    <div>
        match status {
            Status::Ok => <span class="ok">ok</span>,
            Status::Pending => <span class="pending">pending</span>,
            Status::Failed(code) => {
                <span class="failed">failed</span>
                <code>{code}</code>
            }
        }
    </div>
}

pub fn WithLet(first: &str, second: &str) -> Element {
    let joined = format!("{first} and {second}");
    <p>{joined}</p>
}

pub fn Nested(groups: &[Vec<String>], show_empty: bool) -> Element {
    <div>
        for group in groups {
            <ul>
                for item in group {
                    if !item.is_empty() {
                        <li>{item}</li>
                    }
                }
            </ul>
        }
        if show_empty {
            <p class="hint">some were hidden</p>
        }
    </div>
}

pub fn Comparison(a: usize, b: usize) -> Element {
    <div>
        if a < b {
            <span>less</span>
        }
    </div>
}

pub struct Row {
    pub id: u32,
    pub name: String,
}

/// Owned fields reached through a shared reference must be borrowed, not moved.
pub fn Rows(rows: &[Row]) -> Element {
    <table>
        for row in rows {
            <tr data-id={row.id}><td>{row.name}</td></tr>
        }
    </table>
}
