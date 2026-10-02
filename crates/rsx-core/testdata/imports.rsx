use super::shared::button::Button;
use super::shared::card::Card;

pub fn Home(title: &str) -> Element {
    <div class="page">
        <Card title={title} />
        <Button label="Click me" />
    </div>
}
