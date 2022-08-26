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
}

impl CommandOutput {
    pub fn object<T: Serialize>(value: T) -> Result<Self> {
        Ok(CommandOutput::Object(serde_json::to_value(value)?))
    }

    pub fn print(&self, json_output: bool) -> Result<()> {
        if json_output {
            self.print_json()?;
        } else {
            self.print_plain()?;
        }
        Ok(())
    }

    fn print_json(&self) -> anyhow::Result<()> {
        match self {
            CommandOutput::NoOutput => println!("null"),
            CommandOutput::Table {
                columns,
                values,
                summary: _,
                header: _,
            } => crate::table::print_json_table(columns, values)?,
            CommandOutput::Object(value) => println!("{}", serde_json::to_string_pretty(&value)?),
        }
        Ok(())
    }

    fn print_plain(&self) -> anyhow::Result<()> {
        match self {
            CommandOutput::NoOutput => {},
            CommandOutput::Table {
                columns,
                values,
                summary,
                header,
            } => {
                if let Some(txt) = header {
                    println!("{}", txt);
                }
                crate::table::print_table(columns, values, summary)
            },
            CommandOutput::Object(value) => {
                match value {
                    serde_json::Value::String(s) => {
                        println!("{}", s);
                    }
                    value => println!("{}", serde_yaml::to_string(&value)?),
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
