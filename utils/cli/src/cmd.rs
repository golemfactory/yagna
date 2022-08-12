use anyhow::Result;
use serde::Serialize;

pub enum CommandOutput {
    NoOutput,
    Object(serde_json::Value),
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
                    crate::table::print_json_table(columns, values)
                } else {
                    if let Some(txt) = header {
                        println!("{}", txt);
                    }
                    crate::table::print_table(columns, values, summary);
                }
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

impl From<()> for CommandOutput {
    fn from(_: ()) -> Self {
        CommandOutput::NoOutput
    }
}

pub trait FormattedObject {
    fn to_json(&self) -> Result<serde_json::Value>;

    fn print(&self) -> Result<()>;
}
