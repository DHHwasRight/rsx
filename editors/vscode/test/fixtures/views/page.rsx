use super::card::Card;

pub fn Page(user: &User) -> Element {
    <div class="page">
        <Card title={&user.name} count={user.posts.len()} />
    </div>
}
