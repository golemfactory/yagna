use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

pub enum CommandOutput {
    NoOutput,
    Object(serde_json::Value),
    Table {
        columns: Vec<String>,
        values: Vec<serde_json::Value>,
        summary: Vec<serde_json::Value>,
        header: Option<String>,
    }
}

impl CommandOutput {
    pub fn object<T: Serialize>(value: T) -> Result<Self> {
        Ok(CommandOutput::Object(serde_json::to_value(value)?))
    }

    pub fn print(&self, json_output: bool) -> Result<()> {
        match self {
            CommandOutput::NoOutput => {
                if json_output {
                    print_json_null();
                }
            }
            CommandOutput::Table {
                columns,
                values,
                summary,
                header,
            } => {
                if json_output {
                    print_json_table(columns, values)?;
                } else {
                    print_plain_table(columns, values, summary, header)?;
                }
            }
            CommandOutput::Object(value) => {
                if json_output {
                    print_json_output(value)?;
                } else {
                    print_plain_output(value)?;
                }
            }
        }
        Ok(())
    }
}

impl From<()> for CommandOutput {
    fn from(_: ()) -> Self {
        CommandOutput::NoOutput
    }
}

fn print_json_null() {
    println!("null");
}

fn print_json_table(
    columns: &Vec<String>,
    values: &Vec<Value>,) -> Result<()> {
    Ok(crate::table::print_json_table(columns, values)?)
}

fn print_plain_table(
    columns: &Vec<String>,
    values: &Vec<Value>,
    summary: &Vec<Value>,
    header: &Option<String>,
) -> Result<()> {
    if let Some(txt) = header {
        println!("{}", txt);
    }
    Ok(crate::table::print_table(columns, values, summary))
}

fn print_json_output(value: &Value) -> Result<()> {
    Ok(println!("{}", serde_json::to_string_pretty(&value)?))
}

fn print_plain_output(value: &Value) -> Result<()> {
    match value {
        serde_json::Value::String(s) => {
            println!("{}", s);
        }
        value => println!("{}", serde_yaml::to_string(&value)?),
    }
    Ok(())
}