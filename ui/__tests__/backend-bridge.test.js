const childProcess = require('child_process');
const fs = require('fs');

jest.mock('child_process', () => {
  const spawnMock = jest.fn();
  const spawnSyncMock = jest.fn();
  return {
    spawn: spawnMock,
    spawnSync: spawnSyncMock,
  };
});

jest.mock('fs', () => ({
  existsSync: jest.fn(),
}));

const BackendBridge = require('../lib/backend-bridge');

describe('BackendBridge#getPythonCommand', () => {
  beforeEach(() => {
    childProcess.spawnSync.mockReset();
    fs.existsSync.mockReset();
    fs.existsSync.mockReturnValue(false);
  });

  test('returns the first interpreter that responds', () => {
    // Mock backend file exists to prevent constructor from throwing
    fs.existsSync.mockImplementation((path) => {
      return path.includes('backend/main.py');
    });

    childProcess.spawnSync.mockReturnValue({
      status: 0,
      stdout: 'Python 3.9.0',
      stderr: ''
    });
    const bridge = new BackendBridge();
    expect(bridge.getPythonCommand()).toBe('python');
    expect(childProcess.spawnSync).toHaveBeenCalledWith('python', ['--version'], expect.objectContaining({ shell: true }));
  });

  test('falls back through candidates until one succeeds', () => {
    // Mock backend file exists to prevent constructor from throwing
    fs.existsSync.mockImplementation((path) => {
      return path.includes('backend/main.py');
    });

    childProcess.spawnSync
      .mockReturnValueOnce({ status: 1 })
      .mockReturnValueOnce({
        status: 0,
        stdout: 'Python 3.10.0',
        stderr: ''
      });
    const bridge = new BackendBridge();
    expect(bridge.getPythonCommand()).toBe('py');
    expect(childProcess.spawnSync).toHaveBeenNthCalledWith(1, 'python', ['--version'], expect.objectContaining({ shell: true }));
    expect(childProcess.spawnSync).toHaveBeenNthCalledWith(2, 'py', ['--version'], expect.objectContaining({ shell: true }));
  });
});
