use ya_exe_framework::{ExeUnit, ExeUnitBuilder};

use std::path::{Path, PathBuf};

use wasmtime::*;
use wasmtime_wasi::create_wasi_instance;
use wasi_common::preopen_dir;

use anyhow::{bail, Context, Result, Error};
use std::collections::HashMap;
use std::fs::{read, File};


struct DirectoryMount {
    host: PathBuf,
    guest: PathBuf,
}


pub struct WasmtimeFactory;


pub struct Wasmtime {
    store: Store,
    mounts: Vec<DirectoryMount>,
    module_registry: HashMap<String, Instance>,
}


impl Wasmtime {
    pub fn new() -> Box<dyn ExeUnit> {
        let wasmtime = Wasmtime {
            store: Store::default(),
            mounts: vec![],
            module_registry: HashMap::<String, Instance>::new()
        };

        Box::new(wasmtime)
    }
}

impl ExeUnitBuilder for WasmtimeFactory {
    fn create(&self) -> Result<Box<dyn ExeUnit>> {
        Ok(Wasmtime::new())
    }
}

impl WasmtimeFactory {
    pub fn new() -> Box<dyn ExeUnitBuilder> {
        Box::new(WasmtimeFactory{})
    }
}

impl ExeUnit for Wasmtime {
    fn on_deploy(&mut self) -> Result<()> {
        //TODO: Get from external world
        let args = vec!["wasm-binary".to_string()];

        //TODO: Get from external world
//        let dirs_mapping = vec![
//            DirectoryMount {
//                host: cmdargs.input_dir,
//                guest: PathBuf::from("/in/"),
//            },
//            DirectoryMount {
//                host: cmdargs.output_dir,
//                guest: PathBuf::from("/out/"),
//            },
//        ];
//        self.mounts = dirs_mapping;

        self.create_wasi_module(args)?;
        self.load_binary(&PathBuf::from(""))?;
        Ok(())
    }

    fn on_start(&mut self) -> Result<()> {
        // This step does nothing.
        Ok(())
    }

    fn on_transferred(&mut self) -> Result<()> {
        // In current implementation do nothing.
        Ok(())
    }

    fn on_run(&mut self) -> Result<()> {

        match self.module_registry.get("main") {
            Some(instance) => {
                let answer = instance
                    .find_export_by_name("_start")
                    .unwrap()
                    .func()
                    .unwrap();
                let _result = answer.borrow().call(&[])?;
                Ok(())
            },
            None => {
                Err(Error::msg(format!("Module not found")))
            }
        }
    }

    fn on_stop(&mut self) -> Result<()> {
        unimplemented!();
    }
}

impl Wasmtime {

    fn create_wasi_module(&mut self, args: Vec<String>) -> Result<()> {
        let preopen_dirs = Wasmtime::compute_preopen_dirs(&self.mounts)?;
        let wasi_unstable = create_wasi_instance(&self.store, &preopen_dirs, &args, &vec![])
            .expect("Failed to create wasi module.");

        self.module_registry.insert("wasi_unstable".to_owned(), wasi_unstable);
        Ok(())
    }

    fn load_binary(&mut self, binary_file: &Path) -> Result<()> {
        let wasm_binary = read(binary_file)
            .map_err(|error| {
                    Error::msg(format!("Can't load wasm binary {}. Error: {}",
                        binary_file.display(),
                        error))
                })?;

        let mut module = Module::new(&self.store, &wasm_binary)
            .map_err(|error| {
                Error::msg(format!("WASM module creation failed. Error: {}", error))
            })?;

        let imports = self.resolve_imports(&mut module)?;

        let instance = Instance::new(&self.store, &module, &imports)
            .map_err(|error| {
                Error::msg(format!("WASM instance creation failed. Error: {}", error))
            })?;

        self.module_registry.insert("main".to_string(), instance);
        Ok(())
    }

    fn resolve_imports(&self, module: &mut Module) -> Result<Vec<Extern>> {
        Ok(module.imports()
            .iter()
            .map(|import| {
                let module_name = import.module();
                if let Some(instance) = self.module_registry.get(module_name) {
                    let field_name = import.name();
                    if let Some(export) = instance.find_export_by_name(field_name) {
                        Ok(export.clone())
                    } else {
                        bail!(
                            "Import {} was not found in module {}",
                            field_name,
                            module_name
                        )
                    }
                } else {
                    bail!("Import module {} was not found", module_name)
                }
            })
            .collect::<Result<Vec<_>, _>>()?)
    }

    fn compute_preopen_dirs(dirs: &Vec<DirectoryMount>) -> Result<Vec<(String, File)>> {
        let mut preopen_dirs = Vec::new();

        for DirectoryMount { guest, host } in dirs.iter() {
            println!("Mounting: {}::{}", host.display(), guest.display());

            preopen_dirs.push((
                guest.as_os_str().to_str().unwrap().to_string(),
                preopen_dir(host)
                    .with_context(|| format!("failed to open directory '{}'", host.display()))?,
            ));
        }

        Ok(preopen_dirs)
    }
}

