pub struct Row {
    pub id: u32,
    pub name: String,
    pub note: String,
}

pub fn Table(rows: &[Row]) -> Element {
    <table class="listing">
        <thead>
            <tr><th>id</th><th>name</th><th>note</th></tr>
        </thead>
        <tbody>
            for row in rows {
                <tr class="row" data-id={row.id}>
                    <td>{row.id}</td>
                    <td class="name">{row.name}</td>
                    <td>{row.note}</td>
                </tr>
            }
        </tbody>
    </table>
}
