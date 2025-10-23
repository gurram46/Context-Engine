const childProcess = require('child_process');

jest.mock('child_process', () => {
  const spawnMock = jest.fn();
  const spawnSyncMock = jest.fn();
  return {
    spawn: spawnMock,
    spawnSync: spawnSyncMock,
  };
});

const BackendBridge = require('../lib/backend-bridge');

describe('BackendBridge#getPythonCommand', () => {
  beforeEach(() => {
    childProcess.spawnSync.mockReset();
  });

  test('returns the first interpreter that responds', () => {
    childProcess.spawnSync.mockReturnValue({ status: 0 });
    const bridge = new BackendBridge();
    expect(bridge.getPythonCommand()).toBe('python');
    expect(childProcess.spawnSync).toHaveBeenCalledWith('python', ['--version'], expect.any(Object));
  });

  test('falls back through candidates until one succeeds', () => {
    childProcess.spawnSync
      .mockReturnValueOnce({ status: 1 })
      .mockReturnValueOnce({ status: 0 });
    const bridge = new BackendBridge();
    expect(bridge.getPythonCommand()).toBe('py');
    expect(childProcess.spawnSync).toHaveBeenNthCalledWith(1, 'python', ['--version'], expect.any(Object));
    expect(childProcess.spawnSync).toHaveBeenNthCalledWith(2, 'py', ['--version'], expect.any(Object));
  });
});
