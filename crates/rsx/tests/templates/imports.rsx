//! A component used from another module in the same tree.

use super::shared::button::Button;

pub fn Toolbar() -> Element {
    <div class="toolbar">
        <Button label="Save" />
        <Button label="Cancel" />
    </div>
}
