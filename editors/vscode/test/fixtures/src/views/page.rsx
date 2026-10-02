use super::card::Card;

pub fn Page(title: &str) -> Element {
    <div class="page">
        <Card title={title} count={1} />
    </div>
}
