use crate::cmd::CommandOutput;
use prettytable::{color, format, format::TableFormat, Attr, Cell, Row, Table};
use std::collections::HashMap;

pub fn print_table(
    columns: &Vec<String>,
    values: &Vec<serde_json::Value>,
    summary: &Vec<serde_json::Value>,
) {
    let mut table = Table::new();
    table.set_format(*FORMAT_BASIC);

    table.set_titles(Row::new(
        columns
            .iter()
            .map(|c| {
                Cell::new(c)
                    .with_style(Attr::Bold)
                    .with_style(Attr::ForegroundColor(color::GREEN))
            })
            .collect(),
    ));
    if values.is_empty() {
        let _ = table.add_row(columns.iter().map(|_| Cell::new("")).collect());
    }
    for row in values {
        if let Some(row_items) = row.as_array() {
            use serde_json::Value;

            let row_strings = row_items
                .iter()
                .map(|v| match v {
                    Value::String(s) => s.to_string(),
                    Value::Null => "".into(),
                    v => v.to_string(),
                })
                .collect();
            table.add_row(row_strings);
        }
    }
    if !summary.is_empty() {
        table.add_row(Row::empty());
        table.add_empty_row();
        let l = summary.len();
        for (idx, row) in summary.into_iter().enumerate() {
            if let Some(row_items) = row.as_array() {
                use serde_json::Value;

                let row_strings = Row::new(
                    row_items
                        .iter()
                        .map(|v| {
                            let c = Cell::new(&match v {
                                Value::String(s) => s.to_string(),
                                Value::Null => "".into(),
                                v => v.to_string(),
                            });

                            if idx == l - 1 {
                                c.with_style(Attr::Bold)
                            } else {
                                c
                            }
                        })
                        .collect(),
                );
                table.add_row(row_strings);
            }
        }
    }
    let _ = table.printstd();
}

pub fn print_json_table(
    columns: &Vec<String>,
    values: &Vec<serde_json::Value>,
) -> Result<(), anyhow::Error> {
    let columns_size_eq_values_size = values.iter().all(|row| match row {
        serde_json::Value::Array(values) => values.len() == columns.len(),
        _ => false,
    });
    if columns_size_eq_values_size {
        let kvs: Vec<HashMap<&String, &serde_json::Value>> = values
            .iter()
            .map(|row| match row {
                serde_json::Value::Array(row_values) if columns.len() == row_values.len() => {
                    columns
                        .into_iter()
                        .enumerate()
                        .map(|(idx, key)| (key, &row_values[idx]))
                        .collect()
                }
                _ => unreachable!(),
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!(kvs))?)
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "headers": columns,
                "values": values
            }))?
        )
    }
    Ok(())
}

pub struct ResponseTable {
    pub columns: Vec<String>,
    pub values: Vec<serde_json::Value>,
}

impl ResponseTable {
    pub fn sort_by(mut self, arg_key: &Option<impl AsRef<str>>) -> Self {
        let key = match arg_key {
            None => return self,
            Some(k) => k.as_ref(),
        };
        let idx =
            match self
                .columns
                .iter()
                .enumerate()
                .find_map(|(idx, v)| if v == key { Some(idx) } else { None })
            {
                None => return self,
                Some(idx) => idx,
            };
        self.values
            .sort_by_key(|v| Some(v.as_array()?.get(idx)?.to_string()));
        self
    }

    pub fn with_summary(self, summary: Vec<serde_json::Value>) -> CommandOutput {
        CommandOutput::Table {
            columns: self.columns,
            values: self.values,
            summary,
            header: None,
        }
    }

    pub fn with_header(self, header: String) -> CommandOutput {
        CommandOutput::Table {
            columns: self.columns,
            values: self.values,
            summary: Vec::new(),
            header: Some(header),
        }
    }
}

impl From<ResponseTable> for CommandOutput {
    fn from(table: ResponseTable) -> Self {
        CommandOutput::Table {
            columns: table.columns,
            values: table.values,
            summary: Vec::new(),
            header: None,
        }
    }
}

lazy_static::lazy_static! {
    pub static ref FORMAT_BASIC: TableFormat = format::FormatBuilder::new()
        .column_separator('│')
        .borders('│')
        .separators(
            &[format::LinePosition::Top],
            format::LineSeparator::new('─', '┬', '┌', '┐')
        )
        .separators(
            &[format::LinePosition::Title],
            format::LineSeparator::new('─', '┼', '├', '┤')
        )
        .separators(
            &[format::LinePosition::Bottom],
            format::LineSeparator::new('─', '┴', '└', '┘')
        )
        .padding(2, 2)
        .build();
}
