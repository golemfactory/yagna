REM Uncomment the following if DEV TestBed instance is required, otherwise use docker...
REM echo Starting the Market API TestBed...
REM start dotnet run -p ../../../../golem-client-mock/GolemClientMockAPI
REM timeout 20

docker run -d -p 5001:5001 --name testbed golem-client-mock
docker start testbed 

start start_net_mk1_hub.bat
start start_provider_daemon.bat
start start_requestor_daemon.bat
