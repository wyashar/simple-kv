TODO!


1.) Send respones back to client for del and get requests
2.) Multi-client connections
3.) Persist the kv store on client disconnect
4.) Read/write timeouts
5.) Metrics and health endpoints
6.) Unit testing kv server
7.) Integration testings
8.) Better way to test req=>res, maybe some kind of e2e tests in rust instead of the bash script
9.) Send errors back to client
10.) Right now connection closure logs with a misleading parsing error, need to differeniate EOF and bad requests
11.) Stop killing connection on a bad request
12.) Instead of have a From<KVResponse> for Vec<u8> and a From<KVRequest> for Vec<u8>, we can just introduce write functions
  that can write to any W where Write, so we can be a zero-copy implementation
