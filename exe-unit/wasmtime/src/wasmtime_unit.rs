use crate::entrypoint::DirectoryMount;

use wasmtime::*;
use wasi_common::preopen_dir;
use wasmtime_wasi::{
    old::snapshot_0::create_wasi_instance as create_wasi_instance_snapshot_0,
};

use anyhow::{bail, Context, Result, Error};
use log::info;
use std::collections::HashMap;
use std::fs::{read, File};
use std::path::{Path, PathBuf, Component};
use std::{ffi::OsStr};




pub struct Wasmtime {
    store: Store,
    mounts: Vec<DirectoryMount>,
    /// Wasi modules instances and other dependencies, that we can add in future.
    dependencies: HashMap<String, Instance>,
    /// Modules loaded by user.
    modules: HashMap<String, Module>,
}


impl Wasmtime {
    pub fn new(mounts: Vec<DirectoryMount>) -> Wasmtime {
        let wasmtime = Wasmtime {
            store: Store::default(),
            mounts,
            dependencies: HashMap::<String, Instance>::new(),
            modules: HashMap::<String, Module>::new(),
        };

        wasmtime
    }
}


impl Wasmtime {
    pub fn deploy(&mut self, wasm_binary: &Path) -> Result<()> {
        // Loading binary will validate if it can be correctly loaded by wasmtime.
        self.load_binary(wasm_binary)?;
        Ok(())
    }

    pub fn run(&mut self, args: Vec<String>) -> Result<()> {

        if args.len() < 1 {
            return Err(Error::msg(format!("Run command not specified.")));
        }

        let args = Wasmtime::prepare_args(&args);

        self.create_wasi_module(&args)?;
        let instance = self.create_instance(&args[0])?;
        Ok(Wasmtime::run_instance(&instance, "_start")?)
    }

    fn run_instance(instance: &Instance, entrypoint: &str) -> Result<()> {
        info!("Running wasm binary entrypoint {}", entrypoint);

        let function = instance
            .find_export_by_name("_start")
            .with_context(|| format!("Can't find {} entrypoint.", entrypoint))?
            .func()
            .with_context(|| format!("Can't find {} entrypoint.", entrypoint))?;
        //TODO: Return error code from execution.
        let _result = function.borrow().call(&[])?;
        Ok(())
    }

    fn create_wasi_module(&mut self, args: &Vec<String>) -> Result<()> {
        info!("Loading wasi.");

        let preopen_dirs = Wasmtime::compute_preopen_dirs(&self.mounts)?;

        // Create and instantiate snapshot0 of WASI ABI (FWIW, this is one *can* still
        // be targeted when using an older Rust toolchain)
        let snapshot0 = create_wasi_instance_snapshot_0(
            &self.store,
            &preopen_dirs,
            args,
            &vec![],
        )
        .with_context(|| format!("Failed to create snapshot0 WASI module."))?;

        // Create and instantiate snapshot1 of WASI ABI, aka the "current stable"
        let snapshot1 =
            wasmtime_wasi::create_wasi_instance(&self.store, &preopen_dirs, args, &vec![])
                .with_context(|| format!("Failed to create snapshot1 WASI module."))?;

        self.dependencies.insert("wasi_unstable".to_owned(), snapshot0);
        self.dependencies.insert("wasi_snapshot_preview1".to_owned(), snapshot1);
        Ok(())
    }

    fn load_binary(&mut self, binary_file: &Path) -> Result<()> {
        info!("Loading wasm binary: {}", binary_file.display());

        let wasm_binary = read(binary_file)
            .with_context(|| format!("Can't load wasm binary {}.", binary_file.display()))?;

        let module = Module::new(&self.store, &wasm_binary)
            .with_context(|| format!("WASM module creation failed."))?;

        self.modules.insert(Wasmtime::get_module_name(binary_file), module);
        Ok(())
    }

    fn create_instance(&mut self, module_name: &str) -> Result<Instance> {
        info!("Resolving [{}] module's dependencies.", module_name);

        match self.modules.get_mut(module_name) {
            Some(mut module) => {

                let imports = Wasmtime::resolve_imports(&self.dependencies, &mut module)?;
                let instance = Instance::new(&self.store, &module, &imports)
                    .with_context(|| format!("WASM instance creation failed."))?;
                Ok(instance)
            },
            None => return Err(Error::msg(format!("Module {} is not loaded. Did you forgot to run deploy step.", module_name)))
        }
    }

    fn resolve_imports(dependencies: &HashMap<String, Instance>,
                       module: &mut Module) -> Result<Vec<Extern>> {
        Ok(module.imports()
            .iter()
            .map(|import| {
                let module_name = import.module();
                if let Some(instance) = dependencies.get(module_name) {
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
            info!("Mounting: {}::{}", host.display(), guest.display());

            preopen_dirs.push((
                guest.as_os_str().to_str().unwrap().to_string(),
                preopen_dir(host)
                    .with_context(|| format!("Failed to open directory '{}'", host.display()))?,
            ));
        }

        Ok(preopen_dirs)
    }

    fn prepare_args(args: &Vec<String>) -> Vec<String> {
        let mut new_args = Vec::new();

        // Translate binary path to module name, to avoid leaking path information.
        let binary_path = PathBuf::from(args[0].as_str());
        let module_name = Wasmtime::get_module_name(&binary_path);

        new_args.push(module_name);

        for arg in args[1..].iter() {
            new_args.push(arg.clone());
        }

        return new_args;
    }

    fn get_module_name(binary_path: &Path) -> String {
        binary_path
            .components()
            .next_back()
            .map(Component::as_os_str)
            .and_then(OsStr::to_str)
            .unwrap_or("module")
            .to_owned()
    }
}

