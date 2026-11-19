import http from 'node:http';
http.createServer((req,res)=>setTimeout(()=>res.end('ok'),200)).listen(4977,'127.0.0.1',()=>console.log('up'));
