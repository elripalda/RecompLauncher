// Local-only static fixture server for visual QA. No application API or user data.
const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
const types = {'.html':'text/html','.js':'text/javascript','.css':'text/css','.woff2':'font/woff2','.png':'image/png'};
http.createServer((req,res)=>{
  const url = new URL(req.url,'http://127.0.0.1');
  const file = path.resolve(root,'.'+decodeURIComponent(url.pathname));
  if(!file.startsWith(root+path.sep)){res.writeHead(403);return res.end();}
  fs.readFile(file,(err,data)=>{res.writeHead(err?404:200,{'Content-Type':types[path.extname(file)]||'text/plain'});res.end(err?'Not found':data);});
}).listen(4783,'127.0.0.1',()=>process.stdout.write('Preview: http://127.0.0.1:4783/tests/setup-inputs-preview.html\n'));
