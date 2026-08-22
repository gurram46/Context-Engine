import subprocess, os, sys, pathlib
workdir = pathlib.Path.cwd()
# create a small test file in workdir (CommonJS style, uses ts-node/register)
test_code = """
require('tsconfig-paths').register();
require('ts-node').register({ transpileOnly: true });
const { Reflector } = require('./packages/core/services/reflector.service.ts');
const reflector = new Reflector();
const Roles = Reflector.createDecorator();
class MyClass {}
Roles('admin')(MyClass);
class MyHandler {}
Roles(false)(MyHandler);
const result = reflector.getAllAndOverride(Roles, [MyHandler, MyClass]);
if (result !== false) {
  console.error('FAIL: expected false but got ' + String(result));
  process.exit(1);
}
console.log('pass');
"""
test_file = workdir / "_test_reflector.js"
test_file.write_text(test_code, encoding="utf-8")
env = os.environ.copy()
env["NODE_PATH"] = r"C:\Users\Dell\context\Context-Engine\bench\repos\nestjs\node_modules"
# Use node with ts-node/register via NODE_PATH, no loader needed
cmd = ["node", str(test_file)]
proc = subprocess.run(cmd, cwd=str(workdir), capture_output=True, text=True, timeout=30, env=env)
print(proc.stdout)
print(proc.stderr, file=sys.stderr)
# cleanup
try:
    test_file.unlink()
except:
    pass
sys.exit(proc.returncode)
