echo Starting the Market API TestBed...
start dotnet run -p ../../../../golem-client-mock/GolemClientMockAPI
timeout 20
start start_net_mk1_hub.bat
start start_provider_daemon.bat
start start_requestor_daemon.bat
