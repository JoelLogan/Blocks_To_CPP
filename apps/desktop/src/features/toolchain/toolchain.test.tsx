import type {
  Diagnostic,
  Distro,
  Empty,
  Platform,
  Toolchain,
  ToolchainAddDialogResponse,
  ToolchainListResponse,
} from '@blocks2cpp/ipc-types';
import { IpcCallError } from '@blocks2cpp/ipc-types';
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { resetAppStore, useAppStore } from '../../app/store';
import { appInfoFixture, projectFixture, toolchainFixture } from '../../app/testing/fixtures';
import { expectNoAxeViolations } from '../../test/axe';
import {
  deferred,
  featureContext,
  renderScreen,
  settle,
  type TestFeatureContext,
} from '../settings/page/testing';
import { toolchainFeature } from './feature';
import { INSTALL_COMMANDS } from './instructions';
import { failureText, LINK_ADDRESSES, pageTitle } from './ToolchainPage';

/** A toolchain diagnostic. */
function problem(code: string, message: string, severity: Diagnostic['severity'] = 'error') {
  return {
    code,
    severity,
    message,
    primary: { module: null, block: null, part: { kind: 'whole' } },
    source: 'toolchain',
  } as unknown as Diagnostic;
}

const LINUX_GCC = toolchainFixture({
  id: 'tc_1111111111111111',
  version: '13.3.0',
  target: 'x86_64-linux-gnu',
  flavor: null,
  displayPath: '/usr/bin/g++',
  selected: false,
  capabilities: {
    standards: ['c++17', 'c++20', 'c++23'],
    stdFormat: true,
    sanitizers: true,
    sarif: true,
  },
});

const NEWER_GCC = toolchainFixture({
  id: 'tc_3333333333333333',
  version: '14.1.0',
  target: 'x86_64-linux-gnu',
  flavor: null,
  displayPath: '/usr/local/bin/g++',
  source: 'wellKnown',
  selected: false,
});

const OLD_GCC = toolchainFixture({
  id: 'tc_2222222222222222',
  version: '9.4.0',
  target: 'x86_64-linux-gnu',
  flavor: null,
  displayPath: '/opt/old/bin/g++',
  usable: false,
  selected: false,
  capabilities: { standards: ['c++17'], stdFormat: false, sanitizers: false, sarif: false },
  problems: [problem('B2C-T1004', 'g++ 9.4.0 is too old: Blocks2Cpp needs GCC 11 or newer.')],
});

let uninstall: (() => void) | null = null;

interface InstallOptions {
  platform?: Platform;
  distro?: Distro | null;
  list?: Toolchain[];
  discovering?: boolean;
}

/** Installs the feature as the app would, after the bootstrap read the list. */
function install(options: InstallOptions = {}): TestFeatureContext {
  const created = featureContext();
  const list = options.list ?? [];
  const discovering = options.discovering ?? false;
  created.ipc.toolchainSetupInfo.mockResolvedValue({
    platform: options.platform ?? 'linux',
    noUsableToolchain: !list.some((toolchain) => toolchain.usable),
    distro: options.distro ?? null,
  });
  created.ipc.toolchainList.mockResolvedValue({ toolchains: list, discovering });
  useAppStore.getState().actions.setToolchains({ list, discovering });
  uninstall = toolchainFeature(created.ctx);
  return created;
}

/** Installs the feature and shows its page. */
async function showPage(options: InstallOptions = {}): Promise<TestFeatureContext> {
  const created = install(options);
  renderScreen(created, 'toolchainSetup');
  await settle();
  return created;
}

/** Plays a `toolchainsUpdated` event as the bootstrap delivers it: store first, then the bus. */
function pushToolchains(target: TestFeatureContext, list: Toolchain[], discovering: boolean): void {
  act(() => {
    useAppStore.getState().actions.setToolchains({ list, discovering });
    target.ctx.events.emit({ kind: 'toolchainsUpdated', toolchains: list, discovering });
  });
}

function region(name: string | RegExp): HTMLElement {
  return screen.getByRole('region', { name });
}

beforeEach(() => {
  resetAppStore();
});

afterEach(() => {
  uninstall?.();
  uninstall = null;
});

describe('the setup page', () => {
  it('shows the Windows steps: MSYS2 (recommended) and WinLibs, with their links', async () => {
    const page = await showPage({ platform: 'windows' });

    expect(screen.getByRole('heading', { level: 2, name: 'Set up a C++ compiler' })).toBeTruthy();
    const windows = region('Install g++ on Windows');
    expect(windows.textContent).toContain('MSYS2 (recommended)');
    expect(windows.textContent).toContain('MSYS2 UCRT64');
    expect(windows.textContent).toContain(INSTALL_COMMANDS.msys2);
    expect(windows.textContent).toContain(INSTALL_COMMANDS.winlibs);
    expect(screen.queryByRole('region', { name: 'Install g++ on Linux' })).toBeNull();
    expect(region('What is a compiler?').textContent).toContain('g++');
    expect(
      screen.getByText('On Windows, only a program named g++.exe can be chosen.'),
    ).toBeTruthy();

    page.ipc.openHelpLink.mockResolvedValue({});
    fireEvent.click(screen.getByRole('button', { name: /Open msys2\.org/ }));
    await settle();
    fireEvent.click(screen.getByRole('button', { name: /Open winlibs\.com/ }));
    await settle();
    expect(page.ipc.openHelpLink.mock.calls).toEqual([
      [{ linkId: 'msys2Install' }],
      [{ linkId: 'winlibs' }],
    ]);

    await expectNoAxeViolations(screen.getByTestId('toolchain-page'));
  });

  it.each([
    ['Ubuntu', { id: 'ubuntu', idLike: ['debian'] }, INSTALL_COMMANDS.apt],
    ['Fedora', { id: 'fedora', idLike: [] }, INSTALL_COMMANDS.dnf],
    ['Arch', { id: 'arch', idLike: [] }, INSTALL_COMMANDS.pacman],
  ] as const)('shows only the %s command on Linux', async (_name, distro, command) => {
    await showPage({ platform: 'linux', distro: { id: distro.id, idLike: [...distro.idLike] } });

    const linux = region('Install g++ on Linux');
    const shown = [...linux.querySelectorAll('code')].map((element) => element.textContent);
    expect(shown).toEqual([command]);
    expect(screen.queryByRole('region', { name: 'Install g++ on Windows' })).toBeNull();
    expect(
      screen.queryByText('On Windows, only a program named g++.exe can be chosen.'),
    ).toBeNull();
    await expectNoAxeViolations(screen.getByTestId('toolchain-page'));
  });

  it.each([
    ['an unknown distribution', { id: 'gentoo', idLike: [] }],
    ['no os-release', null],
  ])('shows all three Linux commands for %s', async (_name, distro) => {
    await showPage({ platform: 'linux', distro });

    const linux = region('Install g++ on Linux');
    const codes = linux.querySelectorAll('code');
    expect([...codes].map((code) => code.textContent)).toEqual([
      INSTALL_COMMANDS.apt,
      INSTALL_COMMANDS.dnf,
      INSTALL_COMMANDS.pacman,
    ]);
    expect(linux.textContent).toContain('Debian, Ubuntu and Linux Mint');
    expect(linux.textContent).toContain('Fedora, Red Hat Enterprise Linux and CentOS');
    expect(linux.textContent).toContain('Arch Linux and Manjaro');
  });

  it('falls back to the platform from app_info when the setup information cannot be read', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    useAppStore.getState().actions.setAppInfo(appInfoFixture({ platform: 'windows' }));
    const created = featureContext();
    created.ipc.toolchainSetupInfo.mockRejectedValue(
      new IpcCallError('toolchain_setup_info', { code: 'internal' }),
    );
    uninstall = toolchainFeature(created.ctx);
    renderScreen(created, 'toolchainSetup');
    await settle();

    expect(region('Install g++ on Windows')).toBeTruthy();
  });

  it('shows the steps of both platforms when the platform is not known at all', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const created = featureContext();
    created.ipc.toolchainSetupInfo.mockRejectedValue(
      new IpcCallError('toolchain_setup_info', { code: 'internal' }),
    );
    uninstall = toolchainFeature(created.ctx);
    renderScreen(created, 'toolchainSetup');
    await settle();

    expect(region('Install g++ on Windows')).toBeTruthy();
    expect(region('Install g++ on Linux')).toBeTruthy();
  });

  it('copies a command and says so', async () => {
    const writeText = vi.spyOn(navigator.clipboard, 'writeText').mockResolvedValue();
    await showPage({ platform: 'linux', distro: { id: 'ubuntu', idLike: [] } });

    fireEvent.click(
      screen.getByRole('button', { name: `Copy the command ${INSTALL_COMMANDS.apt}` }),
    );
    await settle();

    expect(writeText).toHaveBeenCalledWith(INSTALL_COMMANDS.apt);
    expect(region('Install g++ on Linux').textContent).toContain('Copied');
  });

  it('says how to copy by hand when the clipboard refuses', async () => {
    vi.spyOn(navigator.clipboard, 'writeText').mockRejectedValue(new Error('NotAllowedError'));
    await showPage({ platform: 'linux', distro: { id: 'arch', idLike: [] } });

    fireEvent.click(screen.getByRole('button', { name: /Copy the command/ }));
    await settle();

    expect(region('Install g++ on Linux').textContent).toContain(
      'Could not copy: select the command and press Ctrl+C',
    );
  });

  it('says a link could not be opened, with its address', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const page = await showPage({ platform: 'windows' });
    page.ipc.openHelpLink.mockRejectedValue(
      new IpcCallError('open_help_link', { code: 'io', kind: 'other' }),
    );

    fireEvent.click(screen.getByRole('button', { name: /Open winlibs\.com/ }));
    await settle();

    expect(screen.getByTestId('toolchain-outcome').textContent).toContain(LINK_ADDRESSES.winlibs);
  });

  it('says that discovery is still running instead of showing the steps', async () => {
    await showPage({ discovering: true });

    expect(screen.getByTestId('toolchain-status').textContent).toContain('Looking for g++');
    expect(screen.getByRole('heading', { level: 2, name: 'C++ compiler' })).toBeTruthy();
    expect(screen.queryByRole('region', { name: 'What is a compiler?' })).toBeNull();
    expect(screen.getByTestId('toolchain-list-empty').textContent).toBe('Looking for compilers…');
  });
});

describe('Rescan', () => {
  it('looks again, shows the result and turns the setup page into the toolchain page', async () => {
    const page = await showPage({ platform: 'linux', distro: { id: 'ubuntu', idLike: [] } });
    const answer = deferred<ToolchainListResponse>();
    page.ipc.toolchainRescan.mockReturnValue(answer.promise);

    const rescan = screen.getByRole('button', { name: /I installed it.*Rescan/ });
    fireEvent.click(rescan);
    await settle();

    expect(useAppStore.getState().toolchains.discovering).toBe(true);
    expect(rescan.getAttribute('aria-disabled')).toBe('true');
    expect(rescan.textContent).toBe('Looking for g++…');
    // The steps stay while the rescan runs, and so does the focus on the button.
    expect(region('What is a compiler?')).toBeTruthy();
    // A second press while it runs does nothing.
    fireEvent.click(rescan);
    expect(page.ipc.toolchainRescan).toHaveBeenCalledTimes(1);

    await act(async () => {
      answer.resolve({ toolchains: [LINUX_GCC, OLD_GCC], discovering: false });
      await answer.promise;
    });
    await settle();

    expect(useAppStore.getState().toolchains).toMatchObject({
      list: [LINUX_GCC, OLD_GCC],
      discovering: false,
    });
    expect(screen.getByTestId('toolchain-outcome').textContent).toContain(
      'Found 2 compilers; 1 can build.',
    );
    expect(screen.getByRole('heading', { level: 2, name: 'C++ compiler' })).toBeTruthy();
    expect(screen.queryByRole('region', { name: 'What is a compiler?' })).toBeNull();
    expect(screen.getByTestId('toolchain-status').textContent).toContain('g++ 13.3.0');
    expect(screen.getByRole('button', { name: 'Rescan' })).toBeTruthy();
    await expectNoAxeViolations(screen.getByTestId('toolchain-page'));
  });

  it('says when it still finds nothing usable', async () => {
    const page = await showPage();
    page.ipc.toolchainRescan.mockResolvedValueOnce({ toolchains: [], discovering: false });
    fireEvent.click(screen.getByTestId('toolchain-rescan'));
    await settle();
    expect(screen.getByTestId('toolchain-outcome').textContent).toContain('Still no g++ found.');

    page.ipc.toolchainRescan.mockResolvedValueOnce({ toolchains: [OLD_GCC], discovering: false });
    fireEvent.click(screen.getByTestId('toolchain-rescan'));
    await settle();
    expect(screen.getByTestId('toolchain-outcome').textContent).toContain(
      'Found 1 compiler, but none can be used.',
    );
  });

  it('reports a failure and puts the list back', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const page = await showPage({ list: [OLD_GCC] });
    page.ipc.toolchainRescan.mockRejectedValue(
      new IpcCallError('toolchain_rescan', { code: 'internal' }),
    );
    page.ipc.toolchainList.mockRejectedValue(
      new IpcCallError('toolchain_list', { code: 'internal' }),
    );

    fireEvent.click(screen.getByTestId('toolchain-rescan'));
    await settle();

    expect(screen.getByTestId('toolchain-outcome').textContent).toContain(
      'Looking for compilers did not work (internal).',
    );
    expect(useAppStore.getState().toolchains).toMatchObject({
      list: [OLD_GCC],
      discovering: false,
    });
    expect(screen.getByTestId('toolchain-rescan').getAttribute('aria-disabled')).toBe('false');
  });
});

describe('Choose g++ manually…', () => {
  it('adds the chosen compiler and reads the list again', async () => {
    const page = await showPage();
    const added = toolchainFixture({
      ...LINUX_GCC,
      source: 'manual',
      displayPath: '/opt/gcc/bin/g++',
    });
    page.ipc.toolchainAddDialog.mockResolvedValue({ status: 'ok', toolchain: added });
    page.ipc.toolchainList.mockResolvedValue({ toolchains: [added], discovering: false });
    const listCalls = page.ipc.toolchainList.mock.calls.length;

    fireEvent.click(screen.getByRole('button', { name: 'Choose g++ manually…' }));
    await settle();

    expect(page.ipc.toolchainAddDialog).toHaveBeenCalledTimes(1);
    expect(page.ipc.toolchainList.mock.calls.length).toBe(listCalls + 1);
    expect(useAppStore.getState().toolchains.list).toEqual([added]);
    expect(screen.getByTestId('toolchain-outcome').textContent).toBe(
      '√Done: Added g++ 13.3.0 from /opt/gcc/bin/g++.',
    );
    expect(within(screen.getByTestId('toolchain-list')).getByText('Chosen by hand')).toBeTruthy();
  });

  it('changes nothing when the dialog is cancelled', async () => {
    const page = await showPage();
    page.ipc.toolchainAddDialog.mockResolvedValue({ status: 'cancelled' });
    const listCalls = page.ipc.toolchainList.mock.calls.length;

    fireEvent.click(screen.getByTestId('toolchain-add'));
    await settle();

    expect(page.ipc.toolchainList.mock.calls.length).toBe(listCalls);
    expect(screen.queryByTestId('toolchain-outcome')).toBeNull();
    expect(screen.getByTestId('toolchain-add').getAttribute('aria-disabled')).toBe('false');
  });

  it('explains a B2C-T1002 refusal and adds nothing', async () => {
    const page = await showPage({ platform: 'windows' });
    page.ipc.toolchainAddDialog.mockRejectedValue(
      new IpcCallError('toolchain_add_dialog', {
        code: 'toolchainRejected',
        diagnostics: [
          problem(
            'B2C-T1002',
            'The compiler C:\\tools\\g++.cmd cannot be used: only .exe programs can be used (never .bat or .cmd scripts).',
          ),
        ],
      }),
    );
    const listCalls = page.ipc.toolchainList.mock.calls.length;

    fireEvent.click(screen.getByTestId('toolchain-add'));
    await settle();

    const outcome = screen.getByTestId('toolchain-outcome');
    expect(outcome.getAttribute('role')).toBe('alert');
    expect(outcome.textContent).toContain(
      'This program cannot be used as the compiler, so nothing was added',
    );
    expect(outcome.textContent).toContain('(B2C-T1002)');
    expect(outcome.textContent).toContain('only .exe programs can be used');
    expect(outcome.textContent).toContain('How to fix: Choose the g++ program itself');
    expect(page.ipc.toolchainList.mock.calls.length).toBe(listCalls);
    expect(useAppStore.getState().toolchains.list).toEqual([]);
    await expectNoAxeViolations(screen.getByTestId('toolchain-page'));
  });

  it('says why an added compiler cannot be used', async () => {
    const page = await showPage();
    const added: ToolchainAddDialogResponse = {
      status: 'ok',
      toolchain: { ...OLD_GCC, source: 'manual' },
    };
    page.ipc.toolchainAddDialog.mockResolvedValue(added);
    page.ipc.toolchainList.mockResolvedValue({ toolchains: [added.toolchain], discovering: false });

    fireEvent.click(screen.getByTestId('toolchain-add'));
    await settle();

    const outcome = screen.getByTestId('toolchain-outcome');
    expect(outcome.textContent).toContain('Added g++ 9.4.0, but it cannot be used to build');
    expect(outcome.textContent).toContain('GCC is too old');
  });

  it('asks to close another dialog first when the backend is busy', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const page = await showPage();
    page.ipc.toolchainAddDialog.mockRejectedValue(
      new IpcCallError('toolchain_add_dialog', { code: 'busy' }),
    );

    fireEvent.click(screen.getByTestId('toolchain-add'));
    await settle();

    expect(screen.getByTestId('toolchain-outcome').textContent).toContain(
      'Another dialog is already open.',
    );
  });
});

describe('the toolchain list', () => {
  it('shows version, target, location, capabilities and health checks', async () => {
    const network = toolchainFixture({
      ...NEWER_GCC,
      id: 'tc_4444444444444444',
      displayPath: '\\\\server\\tools\\g++.exe',
      problems: [problem('B2C-T1020', 'The compiler is on a network folder.', 'warning')],
    });
    await showPage({ list: [LINUX_GCC, OLD_GCC, network] });

    const usable = screen.getByTestId(`toolchain-${LINUX_GCC.id}`);
    expect(within(usable).getByRole('heading', { level: 4, name: 'g++ 13.3.0' })).toBeTruthy();
    const text = usable.textContent;
    expect(text).toContain('Can build');
    expect(text).toContain('x86_64-linux-gnu');
    expect(text).toContain('/usr/bin/g++');
    expect(text).toContain('On the PATH');
    expect(text).toContain('C++17, C++20, C++23');
    expect(text).toContain('std::formatYes');
    expect(text).toContain('SanitizersYes');
    expect(text).toContain('SARIF diagnosticsYes');
    expect(text).toContain('Health checksAll passed');

    const old = screen.getByTestId(`toolchain-${OLD_GCC.id}`);
    expect(old.textContent).toContain('Cannot be used');
    expect(old.textContent).toContain('GCC is too old');
    expect(old.textContent).toContain('(B2C-T1004)');
    expect(old.textContent).toContain('g++ 9.4.0 is too old');
    expect(old.textContent).toContain('How to fix: Install g++ 11 or newer');
    expect(within(old).queryByRole('button', { name: /Select as default/ })).toBeNull();

    const remote = screen.getByTestId(`toolchain-${network.id}`);
    expect(remote.textContent).toContain('Can build');
    expect(remote.textContent).toContain('The compiler is on a network folder');
    expect(remote.textContent).toContain('In a usual install folder');

    await expectNoAxeViolations(screen.getByTestId('toolchain-page'));
  });

  it('explains every rejection code of the setup page', async () => {
    const codes = [
      'B2C-T1004',
      'B2C-T1005',
      'B2C-T1006',
      'B2C-T1007',
      'B2C-T1008',
      'B2C-T1014',
      'B2C-T1020',
    ];
    const broken = toolchainFixture({
      ...OLD_GCC,
      problems: codes.map((code) => problem(code, `message of ${code}`)),
    });
    await showPage({ list: [broken] });

    const item = screen.getByTestId(`toolchain-${broken.id}`);
    for (const title of [
      'GCC is too old',
      'Clang is not supported yet',
      'The installation is broken',
      'Cygwin compiler',
      'The old MinGW from mingw.org',
      "This program does not look like GCC's g++",
      'The compiler is on a network folder',
    ]) {
      expect(item.textContent).toContain(title);
    }
  });

  it('shows hidden characters in a location as placeholders', async () => {
    const tricky = toolchainFixture({ ...LINUX_GCC, displayPath: '/opt/\u202Eexe.++g/bin/g++' });
    await showPage({ list: [tricky] });
    expect(screen.getByTestId(`toolchain-${tricky.id}`).textContent).toContain('⟨U+202E⟩');
  });

  it('marks the default, and says when it cannot be used', async () => {
    const brokenDefault = toolchainFixture({ ...OLD_GCC, selected: true });
    await showPage({ list: [LINUX_GCC, brokenDefault] });

    const item = screen.getByTestId(`toolchain-${brokenDefault.id}`);
    expect(within(item).getByTestId('toolchain-default')).toBeTruthy();
    expect(item.textContent).toContain('B2C-T1022');
  });

  it('selects a default and moves the mark', async () => {
    const page = await showPage({ list: [{ ...LINUX_GCC, selected: true }, NEWER_GCC] });
    const answer = deferred<Empty>();
    page.ipc.toolchainSelect.mockReturnValue(answer.promise);
    page.ipc.toolchainList.mockResolvedValue({
      toolchains: [LINUX_GCC, { ...NEWER_GCC, selected: true }],
      discovering: false,
    });

    const select = screen.getByRole('button', { name: 'Select as default (g++ 14.1.0)' });
    fireEvent.click(select);
    await settle();
    expect(select.textContent).toContain('Selecting…');

    await act(async () => {
      answer.resolve({});
      await answer.promise;
    });
    await settle();

    expect(page.ipc.toolchainSelect).toHaveBeenCalledWith({ toolchainId: NEWER_GCC.id });
    const { list } = useAppStore.getState().toolchains;
    expect(list.map((toolchain) => [toolchain.id, toolchain.selected])).toEqual([
      [LINUX_GCC.id, false],
      [NEWER_GCC.id, true],
    ]);
    expect(
      within(screen.getByTestId(`toolchain-${NEWER_GCC.id}`)).getByTestId('toolchain-default'),
    ).toBeTruthy();
    expect(screen.getByTestId('toolchain-outcome').textContent).toContain(
      'g++ 14.1.0 is now the default compiler.',
    );
    expect(screen.getByTestId('toolchain-status').textContent).toContain('g++ 14.1.0');
    expect(screen.getByRole('button', { name: 'Select as default (g++ 13.3.0)' })).toBeTruthy();
  });

  it('reads the list again when the selected compiler is gone', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const page = await showPage({ list: [LINUX_GCC, NEWER_GCC] });
    page.ipc.toolchainSelect.mockRejectedValue(
      new IpcCallError('toolchain_select', { code: 'unknownToolchain' }),
    );
    page.ipc.toolchainList.mockResolvedValue({ toolchains: [LINUX_GCC], discovering: false });

    fireEvent.click(screen.getByRole('button', { name: 'Select as default (g++ 14.1.0)' }));
    await settle();

    expect(screen.getByTestId('toolchain-outcome').textContent).toContain(
      'That compiler is no longer in the list',
    );
    expect(useAppStore.getState().toolchains.list).toEqual([LINUX_GCC]);
  });
});

describe('the toolchain feature', () => {
  it('reads the list and the setup information when it is installed', async () => {
    const created = install({ list: [], discovering: true });
    await settle();
    expect(created.ipc.toolchainList).toHaveBeenCalledTimes(1);
    expect(created.ipc.toolchainSetupInfo).toHaveBeenCalledTimes(1);
    expect(useAppStore.getState().toolchains.setupInfo).toEqual({
      platform: 'linux',
      noUsableToolchain: true,
      distro: null,
    });
  });

  it('shows the setup page once discovery ends without a usable compiler', async () => {
    const created = install({ list: [], discovering: true });
    await settle();
    expect(useAppStore.getState().ui.screen).toBe('start');

    pushToolchains(created, [OLD_GCC], false);
    expect(useAppStore.getState().ui.screen).toBe('toolchainSetup');

    // Leaving the page keeps it closed while nothing changes.
    act(() => {
      useAppStore.getState().actions.setUi({ screen: 'start' });
    });
    pushToolchains(created, [OLD_GCC], false);
    expect(useAppStore.getState().ui.screen).toBe('start');

    // A usable compiler, and later none again: the page comes back.
    pushToolchains(created, [LINUX_GCC], false);
    pushToolchains(created, [], false);
    expect(useAppStore.getState().ui.screen).toBe('toolchainSetup');
  });

  it('shows the setup page at start when the cached list has nothing usable', async () => {
    install({ list: [], discovering: false });
    await settle();
    expect(useAppStore.getState().ui.screen).toBe('toolchainSetup');
  });

  it('changes nothing while a usable compiler is known', async () => {
    const created = install({ list: [LINUX_GCC], discovering: false });
    await settle();
    pushToolchains(created, [LINUX_GCC, OLD_GCC], false);
    expect(useAppStore.getState().ui.screen).toBe('start');
  });

  it('never replaces a page the person opened', async () => {
    useAppStore.getState().actions.setUi({ screen: 'settings' });
    const created = install({ list: [], discovering: true });
    await settle();
    pushToolchains(created, [], false);
    expect(useAppStore.getState().ui.screen).toBe('settings');
  });

  it('never lets an older list answer overwrite a newer event', async () => {
    const created = featureContext();
    const answer = deferred<ToolchainListResponse>();
    created.ipc.toolchainList.mockReturnValue(answer.promise);
    created.ipc.toolchainSetupInfo.mockResolvedValue({
      platform: 'linux',
      noUsableToolchain: false,
      distro: null,
    });
    useAppStore.getState().actions.setToolchains({ list: [], discovering: true });
    uninstall = toolchainFeature(created.ctx);

    pushToolchains(created, [LINUX_GCC], false);
    await act(async () => {
      answer.resolve({ toolchains: [], discovering: true });
      await answer.promise;
    });
    await settle();

    expect(useAppStore.getState().toolchains).toMatchObject({
      list: [LINUX_GCC],
      discovering: false,
    });
  });

  it('removes its page and stops following the list when uninstalled', async () => {
    const created = install({ list: [], discovering: true });
    await settle();
    uninstall?.();
    uninstall = null;
    expect(created.ctx.screens.screen('toolchainSetup')).toBeNull();
    pushToolchains(created, [], false);
    expect(useAppStore.getState().ui.screen).toBe('start');
  });

  it('goes back to the editor or the start page', async () => {
    await showPage({ list: [LINUX_GCC] });
    act(() => {
      useAppStore.getState().actions.setUi({ screen: 'toolchainSetup' });
    });
    fireEvent.click(screen.getByRole('button', { name: 'Back to the start page' }));
    expect(useAppStore.getState().ui.screen).toBe('start');

    act(() => {
      useAppStore.getState().actions.setProject(projectFixture());
    });
    fireEvent.click(screen.getByRole('button', { name: 'Back to the editor' }));
    expect(useAppStore.getState().ui.screen).toBe('editor');
  });

  it('focuses the page heading when the page opens', async () => {
    await showPage({ list: [LINUX_GCC] });
    await waitFor(() => {
      expect(document.activeElement).toBe(screen.getByRole('heading', { level: 2 }));
    });
  });
});

describe('texts', () => {
  it('names the page by what it is for', () => {
    expect(pageTitle(true)).toBe('Set up a C++ compiler');
    expect(pageTitle(false)).toBe('C++ compiler');
  });

  it('explains failures without any text from the backend', () => {
    expect(failureText('add', 'busy')).toContain('Another dialog is already open');
    expect(failureText('select', 'io')).toBe('The choice could not be saved. Try again.');
    expect(failureText('select', 'internal')).toContain('(internal)');
    expect(failureText('openLink', 'io')).toBe('Your web browser could not be opened.');
    expect(failureText('openLink', 'io', 'msys2Install')).toContain(LINK_ADDRESSES.msys2Install);
    expect(failureText('add', 'transport')).toContain('(transport)');
  });
});
