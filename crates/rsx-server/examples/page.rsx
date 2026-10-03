pub struct User {
    pub name: String,
    pub verified: bool,
}

pub struct Post {
    pub title: String,
    pub views: u64,
}

pub fn Card(post: &Post) -> Element {
    <li class="card">
        <h3>{post.title}</h3>
        <span class="views">{post.views}</span>
    </li>
}

pub fn Page(user: &User, posts: &[Post]) -> Element {
    <!DOCTYPE html>
    <html lang="en">
        <head><title>{user.name}</title></head>
        <body>
            <h1>{user.name}</h1>
            if user.verified {
                <span class="badge">Verified</span>
            }
            <ul class="feed">
                for post in posts {
                    <Card post={post} />
                }
            </ul>
        </body>
    </html>
}
