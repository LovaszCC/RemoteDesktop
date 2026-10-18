import { describe, expect, it } from 'vitest';
import { fakeBridge } from '../testing/bridge';
import { status } from '../testing/fixtures';
import { createAppStore } from './app.svelte';

describe('app store', () => {
  it('loads the status and follows app-status-changed', async () => {
    const bridge = fakeBridge({ app_status: status() });
    const app = createAppStore(bridge);
    expect(app.phase).toBeNull();
    await app.start();
    expect(app.phase).toEqual({ kind: 'locked', needsSetup: false });
    expect(app.platform).toBe('macos');
    bridge.emit(
      'app-status-changed',
      status({ phase: { kind: 'unlocked', readOnly: true }, readOnly: true }),
    );
    expect(app.isUnlocked).toBe(true);
    expect(app.readOnly).toBe(true);
    app.stop();
    expect(bridge.listenerCount('app-status-changed')).toBe(0);
  });

  it('subscribes once', async () => {
    const bridge = fakeBridge({ app_status: status() });
    const app = createAppStore(bridge);
    await app.start();
    await app.start();
    expect(bridge.listenerCount('app-status-changed')).toBe(1);
  });

  it('applies the status returned by unlock and rejects a wrong password', async () => {
    const unlocked = status({ phase: { kind: 'unlocked', readOnly: false } });
    const bridge = fakeBridge({
      unlock: (args?: Record<string, unknown>) => {
        if (args?.password !== 'right')
          throw { code: 'wrongPassword', message: 'Wrong password', detail: null };
        return unlocked;
      },
    });
    const app = createAppStore(bridge);
    await expect(app.unlock('wrong', true)).rejects.toMatchObject({ code: 'wrongPassword' });
    expect(app.phase).toBeNull();
    await app.unlock('right', false);
    expect(bridge.invoke).toHaveBeenLastCalledWith('unlock', { password: 'right', remember: false });
    expect(app.isUnlocked).toBe(true);
  });

  it('chooses a database through the file dialog', async () => {
    const opened = status({ databasePath: '/other.sqlite' });
    const bridge = fakeBridge({
      pick_database_file: (args?: Record<string, unknown>) =>
        args?.mode === 'open' ? '/other.sqlite' : null,
      open_database: opened,
    });
    const app = createAppStore(bridge);
    expect(await app.chooseDatabase('create')).toBeNull();
    expect(await app.chooseDatabase('open')).toEqual(opened);
    expect(bridge.invoke).toHaveBeenLastCalledWith('open_database', { path: '/other.sqlite' });
    expect(app.databasePath).toBe('/other.sqlite');
  });

  it('creates a database at the picked path', async () => {
    const bridge = fakeBridge({ pick_database_file: '/new.sqlite', create_database: status() });
    await createAppStore(bridge).chooseDatabase('create');
    expect(bridge.invoke).toHaveBeenLastCalledWith('create_database', { path: '/new.sqlite' });
  });

  it('resolves a lock conflict and retries', async () => {
    const bridge = fakeBridge({ resolve_db_lock: status(), reopen_database: status() });
    const app = createAppStore(bridge);
    await app.resolveDbLock('openAnyway');
    expect(bridge.invoke).toHaveBeenLastCalledWith('resolve_db_lock', { choice: 'openAnyway' });
    await app.reopenDatabase();
    expect(bridge.invoke).toHaveBeenLastCalledWith('reopen_database', undefined);
  });
});
