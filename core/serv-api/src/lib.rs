use anyhow::Result;
use prettytable::{color, format, format::TableFormat, Attr, Cell, Row, Table};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub struct MetricsCtx {
    pub push_enabled: bool,
    pub push_host_url: Option<url::Url>,
}

#[derive(Clone, Debug, Default)]
pub struct CliCtx {
    pub data_dir: PathBuf,
    pub gsb_url: Option<url::Url>,
    pub json_output: bool,
    pub accept_terms: bool,
    pub metrics_ctx: Option<MetricsCtx>,
}

impl CliCtx {
    pub fn output(&self, output: CommandOutput) {
        output.print(self.json_output)
    }
}

pub enum CommandOutput {
    NoOutput,
    Object(serde_json::Value),
    PlainString(String),
    Table {
        columns: Vec<String>,
        values: Vec<serde_json::Value>,
        summary: Vec<serde_json::Value>,
        header: Option<String>,
    },
    FormattedObject(Box<dyn FormattedObject>),
}

impl CommandOutput {
    pub fn object<T: Serialize>(value: T) -> Result<Self> {
        Ok(CommandOutput::Object(serde_json::to_value(value)?))
    }

    pub fn print(&self, json_output: bool) {
        match self {
            CommandOutput::NoOutput => {
                if json_output {
                    println!("null");
                }
            }
            CommandOutput::Table {
                columns,
                values,
                summary,
                header,
            } => {
                if json_output {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "headers": columns,
                            "values": values
                        }))
                        .unwrap()
                    )
                } else {
                    if let Some(txt) = header {
                        println!("{}", txt);
                    }
                    print_table(columns, values, summary);
                }
            }
            CommandOutput::PlainString(v) => {
                println!("{}", v);
            }
            CommandOutput::Object(v) => {
                if json_output {
                    println!("{}", serde_json::to_string_pretty(&v).unwrap())
                } else {
                    match v {
                        serde_json::Value::String(s) => {
                            println!("{}", s);
                        }
                        v => println!("{}", serde_yaml::to_string(&v).unwrap()),
                    }
                }
            }
            CommandOutput::FormattedObject(formatted_object) => {
                if json_output {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&formatted_object.to_json().unwrap()).unwrap()
                    )
                } else {
                    formatted_object.print().unwrap()
                }
            }
        }
    }
}

fn print_table(
    columns: &Vec<String>,
    values: &Vec<serde_json::Value>,
    summary: &Vec<serde_json::Value>,
) {
    let mut table = Table::new();
    //table.set_format(*format::consts::FORMAT_NO_LINESEP_WITH_TITLE);
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

impl From<()> for CommandOutput {
    fn from(_: ()) -> Self {
        CommandOutput::NoOutput
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

pub trait FormattedObject {
    fn to_json(&self) -> Result<serde_json::Value>;

    fn print(&self) -> Result<()>;
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

#[cfg(feature = "with-awc")]
pub mod awc;
