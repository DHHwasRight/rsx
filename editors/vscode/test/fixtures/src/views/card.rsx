pub struct User {
    pub name: String,
    pub posts: Vec<String>,
}

/// A card with a title.
pub fn Card(title: &str, count: usize) -> Element {
    <div class="card">
        <h2>{title}</h2>
        <span>{count}</span>
    </div>
}

pub fn Panel(children: Element) -> Element {
    <section class="panel">{children}</section>
}

pub fn Profile(user: &User) -> Element {
    <p class="profile">{user.name}</p>
}
