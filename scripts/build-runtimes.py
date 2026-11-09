"""Build the pinned inference workers with the host CMake toolchain."""
import pathlib
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
WORK = ROOT / '.runtime'
PINS = {
    'whisper.cpp': ('d2a764c648779feb36d44363f212a29b00aad1af', 'whisper-server'),
    'llama.cpp': ('a4d4f2fd4ec85ec6a0a9ece4870416c1b6cc24f5', 'llama-server'),
}
for name, (revision, target) in PINS.items():
    source = WORK / name
    if not source.exists():
        subprocess.run(['git', 'clone', '--filter=blob:none', '--no-checkout', f'https://github.com/ggml-org/{name}.git', str(source)], check=True)
    subprocess.run(['git', '-C', str(source), 'checkout', '--detach', revision], check=True)
    if name == 'whisper.cpp':
        server = source / 'examples/server/server.cpp'
        text = server.read_text()
        anchor = '    // store default params so we can reset after each inference request'
        patch = r'''    const char * conch_env = std::getenv("CONCH_WORKER_TOKEN");
    if (!conch_env || !*conch_env) { fprintf(stderr, "Conch worker token missing\n"); return 1; }
    const std::string conch_auth = std::string("Bearer ") + conch_env;
    svr->set_payload_max_length(8 * 1024 * 1024);
    svr->set_pre_routing_handler([&](const Request & req, Response & res) {
        if (req.get_header_value("Authorization") != conch_auth) {
            res.status = 401;
            return httplib::Server::HandlerResponse::Handled;
        }
        if (req.path != "/health" && req.path != "/inference") {
            res.status = 404;
            return httplib::Server::HandlerResponse::Handled;
        }
        return httplib::Server::HandlerResponse::Unhandled;
    });
'''
        if 'conch_env' not in text:
            if text.count(anchor) != 1:
                raise RuntimeError('Pinned whisper-server patch no longer applies')
            server.write_text(text.replace(anchor, patch + anchor))
    subprocess.run(['cmake', '-S', str(source), '-B', str(source / 'build'), '-DCMAKE_BUILD_TYPE=Release', '-DBUILD_SHARED_LIBS=OFF', '-DGGML_NATIVE=OFF', '-DLLAMA_OPENSSL=OFF', '-DCMAKE_OSX_DEPLOYMENT_TARGET=11.0', '-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded'], check=True)
    subprocess.run(['cmake', '--build', str(source / 'build'), '--config', 'Release', '--target', target, '-j', '3'], check=True)
    suffix = '.exe' if sys.platform == 'win32' else ''
    candidates = list((source / 'build').rglob(target + suffix))
    out = ROOT / 'src-tauri' / 'workers'
    out.mkdir(exist_ok=True)
    shutil.copy2(candidates[0], out / (target + suffix))
    for filename in ('LICENSE',):
        shutil.copy2(source / filename, out / (name + '-' + filename))
