use comfy_table::{presets::UTF8_FULL_CONDENSED, Table};

#[derive(Clone, Copy, PartialEq)]
pub enum Format {
    Table,
    Json,
}

impl Format {
    pub fn from_str(s: &str) -> Self {
        match s {
            "json" => Format::Json,
            _ => Format::Table,
        }
    }
}

pub fn print_table(headers: &[&str], rows: Vec<Vec<String>>, format: Format) {
    if format == Format::Json {
        let items: Vec<_> = rows
            .iter()
            .map(|row| {
                headers
                    .iter()
                    .zip(row.iter())
                    .map(|(h, v)| (h.to_string(), serde_json::Value::String(v.clone())))
                    .collect::<serde_json::Map<String, serde_json::Value>>()
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items).unwrap());
        return;
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_header(headers);
    for row in rows {
        table.add_row(row);
    }
    println!("{table}");
}

pub fn print_json<T: serde::Serialize>(value: &T) {
    println!("{}", serde_json::to_string_pretty(value).unwrap());
}
