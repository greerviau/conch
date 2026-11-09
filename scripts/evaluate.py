"""Run synthetic inference fixtures against the pinned local workers."""
import os
import hashlib
import json
import pathlib
import subprocess
import time
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
WORK = ROOT / '.runtime'
SYSTEM = 'You are a text normalizer for speech-to-text transcripts. The input begins with a control line specifying the styling, structure, and context settings; clean the transcript to match those settings and output only the cleaned text.'

def server(name, model, port, extra):
    started = time.monotonic()
    process = subprocess.Popen([str(ROOT / 'src-tauri/workers' / name), '--model', str(WORK / model), '--host', '127.0.0.1', '--port', str(port), '--threads', '4', *extra], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env={**os.environ,"CONCH_WORKER_TOKEN":"synthetic-evaluation"})
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f'{name} exited with {process.returncode}')
        try:
            with urllib.request.urlopen(urllib.request.Request(f'http://127.0.0.1:{port}/health', headers={'Authorization':'Bearer synthetic-evaluation'}), timeout=1):
                process.startup_seconds = round(time.monotonic() - started, 3)
                return process
        except (OSError, TimeoutError):
            time.sleep(.1)
    process.kill()
    raise RuntimeError(f'{name} startup timeout')

results = []
for cpu in (True, False):
    first_result = len(results)
    process = server('llama-server', 's1-mini-q4_k_m.gguf', 18761, ['--ctx-size','4096','--reasoning','off','--chat-template-kwargs','{"enable_thinking":false}','--no-webui', *(['--gpu-layers','0'] if cpu else [])])
    try:
        for text in ('um do not change CONCHPROTECTED0X before 12 tests pass', 'use max groups no wait use min groups and never submit', 'um uh', 'update CONCHPROTECTED0X then CONCHPROTECTED1X without changing 42'):
            payload = {'messages':[{'role':'system','content':SYSTEM},{'role':'user','content':'[Styling: semi-formal] [Structure: prose] [Context: general]\n'+text}], 'temperature':0, 'max_tokens':1024, 'chat_template_kwargs':{'enable_thinking':False}}
            started = time.monotonic()
            request = urllib.request.Request('http://127.0.0.1:18761/v1/chat/completions', data=json.dumps(payload).encode(), headers={'Content-Type':'application/json'})
            with urllib.request.urlopen(request, timeout=60) as response:
                output = json.load(response)['choices'][0]['message']['content']
            results.append({'engine':'s1-mini','cpu':cpu,'input':text,'output':output,'seconds':round(time.monotonic()-started,3)})
    finally:
        process.terminate()
        _, status, usage = os.wait4(process.pid, 0)
        process.returncode = os.waitstatus_to_exitcode(status)
        for result in results[first_result:]:
            result['worker_startup_seconds'] = process.startup_seconds
            result['worker_ru_maxrss'] = usage.ru_maxrss

subprocess.run(['say','-o',str(WORK/'synthetic.aiff'),'Do not change pod G C before twelve tests pass. Set max groups to four.'],check=True)
subprocess.run(['ffmpeg','-y','-i',str(WORK/'synthetic.aiff'),'-ar','16000','-ac','1',str(WORK/'synthetic.wav')],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
for cpu in (True,False):
    first_result = len(results)
    process = server('whisper-server','ggml-tiny.en.bin',18762,['--language','en',*(['--no-gpu'] if cpu else [])])
    try:
        started = time.monotonic()
        output = subprocess.check_output(['curl','--fail','-s','http://127.0.0.1:18762/inference','-H','Authorization: Bearer synthetic-evaluation','-F',f'file=@{WORK / "synthetic.wav"}','-F','response_format=json']).decode()
        results.append({'engine':'whisper-tiny.en','cpu':cpu,'output':json.loads(output),'seconds':round(time.monotonic()-started,3)})
    finally:
        process.terminate()
        _, status, usage = os.wait4(process.pid, 0)
        process.returncode = os.waitstatus_to_exitcode(status)
        for result in results[first_result:]:
            result['worker_startup_seconds'] = process.startup_seconds
            result['worker_ru_maxrss'] = usage.ru_maxrss

(ROOT/'docs/validation/synthetic-inference.json').write_text(json.dumps(results,indent=2)+'\n')
print(json.dumps(results,indent=2))
