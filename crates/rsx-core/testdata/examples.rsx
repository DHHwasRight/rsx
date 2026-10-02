//! A showcase of every feature, as a blog-style application.
//!
//! This is the broadest golden fixture: if a feature regresses, the diff
//! shows up here before it shows up in a hand-written assertion.

use std::collections::BTreeMap;

pub struct User {
    pub name: String,
    pub age: u32,
    pub score: f64,
    pub bio: Option<String>,
    pub role: String,
    pub verified: bool,
}

pub struct Comment {
    pub author: String,
    pub body: String,
}

pub struct Post {
    pub title: String,
    pub comments: Vec<Comment>,
}

// ---------------------------------------------------------------
// Basic HTML and text output
// ---------------------------------------------------------------

pub fn Greeting(name: &str) -> Element {
    <div class="greeting">
        <h1>Hello, {name}!</h1>
        <p>Welcome to the site.</p>
    </div>
}

pub fn Divider() -> Element {
    <hr>
}

// ---------------------------------------------------------------
// Parameters: owned, borrowed, optional
// ---------------------------------------------------------------

pub fn UserBio(user: &User) -> Element {
    <div class="user-bio">
        <h2>{user.name}</h2>
        <span>Age: {user.age}</span>
        <span>Score: {user.score}</span>
        if let Some(bio) = &user.bio {
            <p class="bio">{bio}</p>
        }
        <span class="role">{user.role}</span>
        if user.verified {
            <span class="badge">Verified</span>
        }
    </div>
}

// ---------------------------------------------------------------
// Attributes: static, expression, bare, conditional
// ---------------------------------------------------------------

pub fn SearchForm(action: &str, query: &str, autofocus: bool) -> Element {
    <form action={action} method="post">
        <input type="text" name="q" value={query} placeholder="Search..." autofocus?={autofocus}>
        <button type="submit">Go</button>
    </form>
}

// ---------------------------------------------------------------
// Void and self-closing elements
// ---------------------------------------------------------------

pub fn MediaEmbed(src: &str, poster: &str) -> Element {
    <div class="media">
        <img src={src} alt="thumbnail" />
        <br>
        <source src={src} type="video/mp4">
        <link rel="prefetch" href={src}>
        <meta name="thumbnail" content={poster}>
    </div>
}

// ---------------------------------------------------------------
// Control flow: if / else if / else
// ---------------------------------------------------------------

pub fn RoleBadge(role: &str) -> Element {
    if role == "admin" {
        <span class="badge admin">Admin</span>
    } else if role == "moderator" {
        <span class="badge mod">Moderator</span>
    } else if role == "editor" {
        <span class="badge editor">Editor</span>
    } else {
        <span class="badge member">Member</span>
    }
}

pub fn MaybeAlert(show: bool, message: &str) -> Element {
    if show {
        <div class="alert">{message}</div>
    }
}

pub fn CollapsibleSection(collapsed: bool, title: &str, children: Element) -> Element {
    <div class="section">
        <h3>{title}</h3>
        if !collapsed {
            <div class="section-body">
                {children}
            </div>
        }
    </div>
}

// ---------------------------------------------------------------
// Control flow: match
// ---------------------------------------------------------------

pub fn StatusIcon(status: &str) -> Element {
    match status {
        "success" => <span class="icon green">OK</span>,
        "warning" => <span class="icon yellow">!</span>,
        "error" => <span class="icon red">X</span>,
        _ => <span class="icon gray">?</span>,
    }
}

pub fn PriorityLabel(level: u8) -> Element {
    match level {
        1 => <strong class="p1">Critical</strong>,
        2 => <strong class="p2">High</strong>,
        3 => <em class="p3">Medium</em>,
        _ => {}
    }
}

// ---------------------------------------------------------------
// Iteration
// ---------------------------------------------------------------

pub fn TagList(tags: &[String]) -> Element {
    <ul class="tags">
        for tag in tags {
            <li class="tag">{tag}</li>
        }
    </ul>
}

pub fn NumberedList(items: &[String]) -> Element {
    <ol>
        for (idx, item) in items.iter().enumerate() {
            <li>{idx}. {item}</li>
        }
    </ol>
}

pub fn CommentThread(post: &Post) -> Element {
    <div class="comments">
        for comment in &post.comments {
            <div class="comment">
                <strong>{comment.author}</strong>
                <p>{comment.body}</p>
            </div>
        }
    </div>
}

pub fn FilteredList(items: &[String], show_empty: bool) -> Element {
    <ul>
        for item in items {
            if !item.is_empty() {
                <li>{item}</li>
            }
        }
    </ul>
    if show_empty {
        <p class="hint">Empty items were hidden.</p>
    }
}

// ---------------------------------------------------------------
// Fragments
// ---------------------------------------------------------------

pub fn PageMeta(title: &str, description: &str) -> Element {
    <>
        <meta name="title" content={title}>
        <meta name="description" content={description}>
        <title>{title}</title>
    </>
}

// ---------------------------------------------------------------
// Raw output
// ---------------------------------------------------------------

pub fn RichContent(html: &str) -> Element {
    <div class="prose">
        {!html}
    </div>
}

pub fn InlineCode(code: &str, lang: &str) -> Element {
    <pre class="highlight">
        <code class={lang}>{!code}</code>
    </pre>
}

// ---------------------------------------------------------------
// Spread attributes
// ---------------------------------------------------------------

pub fn Button(label: &str, attrs: &BTreeMap<String, String>) -> Element {
    <button class="btn" { ...attrs }>{label}</button>
}

pub fn Input(base_class: &str, extra: &BTreeMap<String, String>) -> Element {
    <input class={base_class} { ...extra }>
}

// ---------------------------------------------------------------
// Composition
// ---------------------------------------------------------------

pub fn Card(title: &str, subtitle: &str, children: Element) -> Element {
    <div class="card">
        <div class="card-header">
            <h3>{title}</h3>
            if !subtitle.is_empty() {
                <small>{subtitle}</small>
            }
        </div>
        <div class="card-body">
            {children}
        </div>
    </div>
}

pub fn Alert(kind: &str, children: Element) -> Element {
    <div class={format!("alert alert-{kind}")}>
        {children}
    </div>
}

pub fn Dashboard(user_name: &str, notifications: &[String]) -> Element {
    <div class="dashboard">
        <Card title="Welcome" subtitle={format!("Hello, {user_name}")}>
            <p>Your dashboard is ready.</p>
            <Alert kind="success">
                <strong>All systems operational.</strong>
            </Alert>
        </Card>

        <Card title="Notifications" subtitle="">
            if !notifications.is_empty() {
                <ul>
                    for note in notifications {
                        <li>{note}</li>
                    }
                </ul>
            } else {
                <p class="empty">No new notifications.</p>
            }
        </Card>
    </div>
}

// ---------------------------------------------------------------
// Full page layout
// ---------------------------------------------------------------

pub fn HtmlLayout(title: &str, lang: &str, children: Element) -> Element {
    <!DOCTYPE html>
    <html lang={lang}>
        <head>
            <meta charset="utf-8">
            <meta name="viewport" content="width=device-width, initial-scale=1">
            <title>{title}</title>
            <style>body { margin: 0; font-family: sans-serif; }</style>
        </head>
        <body>
            {children}
            <script>if (window.ready && 1 < 2) { init("app"); }</script>
        </body>
    </html>
}

pub fn Nav(links: &[String]) -> Element {
    <nav>
        <ul>
            for link in links {
                <li><a href={link}>{link}</a></li>
            }
        </ul>
    </nav>
}

pub fn Copyright(year: u32) -> Element {
    <footer>
        <p>Copyright {year}. All rights reserved. AT&T &amp; others.</p>
    </footer>
}

// ---------------------------------------------------------------
// A component combining many features at once
// ---------------------------------------------------------------

pub fn BlogPost(
    title: &str,
    author: &str,
    body_html: &str,
    tags: &[String],
    status: &str,
    featured: bool,
    comments: &[String],
) -> Element {
    <article class="post">
        if featured {
            <div class="ribbon">Featured</div>
        }

        <header>
            <h1>{title}</h1>
            <span class="author">by {author}</span>
            <RoleBadge role={status} />
        </header>

        <div class="body">
            {!body_html}
        </div>

        if !tags.is_empty() {
            <TagList tags={tags} />
        }

        <section class="comments">
            <h2>Comments</h2>
            if !comments.is_empty() {
                for (i, comment) in comments.iter().enumerate() {
                    <div class="comment">
                        <span class="num">{i}</span>
                        <p>{comment}</p>
                    </div>
                }
            } else if featured {
                <p>Be the first to comment on this featured post!</p>
            } else {
                <p>No comments yet.</p>
            }
        </section>
    </article>
}
