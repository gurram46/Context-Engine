import { Reflector } from './packages/core/services/reflector.service.ts';
const reflector = new Reflector();
const Roles = Reflector.createDecorator();
class MyClass {}
Roles('admin')(MyClass);
class MyHandler {}
Roles(false)(MyHandler);
const result = reflector.getAllAndOverride(Roles, [MyHandler, MyClass]);
if (result !== false) {
  console.error(`FAIL: expected false but got ${result}`);
  process.exit(1);
}
console.log('pass');
