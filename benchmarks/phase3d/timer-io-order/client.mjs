// Phase 3D: Node.js reference for an expired AbortSignal.timeout vs a response that
// arrived while the event loop was blocked. Run server.mjs, then client.mjs (Node 24).
// Result (benchmarks/results/phase3d/timer-io-order/node.json): both outcomes occur.
const sleep=(ms)=>new Promise(r=>setTimeout(r,ms));
const out={};
for (let i=0;i<5;i++){
 const t0=Date.now(); const signal=AbortSignal.timeout(100);
 const p=fetch('http://127.0.0.1:4977/',{signal}).then(r=>({outcome:'response',status:r.status}),e=>({outcome:'rejected',name:e.name}));
 await sleep(20); const until=Date.now()+600; while(Date.now()<until){}
 const r=await p; out[i]={...r,ms:Date.now()-t0};
}
console.log(JSON.stringify(out));
