//! Минимальная автономная страница без внешних ресурсов.
pub const INDEX: &str = r#"<!doctype html>
<html lang="ru"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>FoxyDrop</title>
<style>
*{box-sizing:border-box}body{margin:0;min-height:100vh;display:flex;justify-content:center;align-items:center;background:#f5f6f8;font:16px system-ui,sans-serif;color:#273142}
main{text-align:center;background:white;padding:38px 24px;border-radius:16px;box-shadow:0 5px 28px #0001;width:min(92vw,400px)}
h1{font-size:28px;margin:0 0 26px}button{background:#ff6600;color:white;border:0;border-radius:8px;padding:14px 30px;font:600 17px system-ui;cursor:pointer}button:disabled{opacity:.55;cursor:wait}
#status{min-height:28px;margin-top:20px;font-size:15px}small{color:#667085;display:block;margin-top:14px}
</style></head><body><main><h1>FoxyDrop</h1><input type="file" id="file" accept=".gpx,.ms,.plt,.kml,.wpt" hidden>
<button id="pick">Загрузить файл</button><div id="status" role="status" aria-live="polite"></div><small>GPX · MS · PLT · KML · WPT</small></main>
<script>
const file=document.getElementById('file'), button=document.getElementById('pick'), status=document.getElementById('status');
let clearTimer;
button.onclick=()=>file.click();
file.onchange=async()=>{
  if(!file.files.length)return;
  clearTimeout(clearTimer); button.disabled=true; status.textContent='Загрузка…';
  try {
    const data=new FormData(); data.append('file',file.files[0]);
    const res=await fetch('/upload',{method:'POST',body:data,cache:'no-store'});
    const json=await res.json();
    status.textContent=json.message||'Сбой загрузки';
  }catch(_){status.textContent='Сбой загрузки';}
  finally{file.value='';button.disabled=false;clearTimer=setTimeout(()=>status.textContent='',3000);}
};
</script></body></html>"#;
