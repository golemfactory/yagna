# Integration Scenario "0"

## Scenario

1) Setup a test network of: 
    - Requestor Daemon 
      - API port `6000`
      - GSB port `6010`
    - Provider Daemon
      - API port `6001`
      - GSB port `6011`
    - Net Mk1 Test Hub
      - Port `7477`
    - Market API Mock TestBed
      - On `http://localhost:5001`
2) Launch sample Agents:
    - Sample Provider Agent
      - With WasmTime ExeUnit
    - Sample Requestor Agent
      - To send a Demand for Wasm computation of `rust-wasi-tutorial` package.

## Test Artefacts

- `local-exeunits-descriptor.json`
  
  A descriptor of ExeUnits to be used by Provider Agent. Note how the paths pointing to the binaries are relative:
  - `"path": "../../../target/debug/exe-unit"` - the ExeUnit "wrapper" binary
  - `"-b", "../../../target/debug/wasmtime-exeunit"` - the WasmTime ExeUnit core module

- `agreement.json`
  
  A mock agreement which shall be passed to the ExeUnit to execute.
  Note: this agreement points to the Wasm package for execution:
    `"task_package": "hash://sha3:38D951E2BD2408D95D8D5E5068A69C60C8238FA45DB8BC841DC0BD50:http://34.244.4.185:8000/rust-wasi-tutorial.zip"`
  
- `exe_script.json`

  The ExeScript to be executed by the Requestor Agent after the Activity is Created during the scenario.

## Setup

Follow these steps to setup the scenario environment for first run:

1) Launch Daemons
   ```
   ./start_daemons.bat
   ```
2) Generate the Requestor and Provider app_keys
   ```
   ./yagnacli_requestor.bat app_key create requestor_key
   ./yagnacli_provider.bat app_key create provider_key
   ```
3) Display the Requestor and Provider app_keys 
   ```
   ./yagnacli_requestor.bat app_key list
   ./yagnacli_provider.bat app_key list
   ```
   
   and copy them to `start_requestor.bat` and `start_provider.bat` respectively.
4) Kill the Daemons and Hub/TestBed processes.


## Run

Follow these steps to run the scenario after initial setup:

1) Start Daemons
   ```
   ./start_daemons.bat
   ```

2) Start Agent apps
   ```
   ./start_apps.bat
   ```
   
