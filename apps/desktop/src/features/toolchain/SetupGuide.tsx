import type { Distro, Platform } from '@blocks2cpp/ipc-types';

import { PageSection } from '../settings/page/FeaturePage';
import type { SetupLink } from './controller';
import { CopyCommand } from './CopyCommand';
import {
  INSTALL_COMMANDS,
  instructionPlatforms,
  linuxCommands,
  PACKAGE_MANAGER_DISTROS,
} from './instructions';

/** The visible label of the rescan button, which the steps refer to. */
export const RESCAN_LABEL = 'I installed it → Rescan';

/** The props of {@link SetupGuide}. */
export interface SetupGuideProps {
  /** The platform, or `null` when it is not known (both platforms are shown). */
  platform: Platform | null;
  /** The Linux distribution, or `null` when it is not known (every Linux command is shown). */
  distro: Distro | null;
  /** Opens an install page in the system browser. */
  onOpenLink: (link: SetupLink) => void;
  /** Whether another action runs, so the link buttons wait. */
  busy: boolean;
}

/**
 * The friendly part of the setup page (docs/spec/04-user-interface.md §4.6): what a compiler is,
 * and how to install g++ on this platform. Nothing is downloaded or bundled by Blocks2Cpp
 * (10 §10.3 Q5); the person installs the compiler themselves.
 */
export function SetupGuide({ platform, distro, onOpenLink, busy }: SetupGuideProps) {
  const platforms = instructionPlatforms(platform);
  return (
    <>
      <PageSection title="What is a compiler?" testId="setup-what">
        <p>
          Your blocks are turned into C++ code, which you can read in the C++ panel. A{' '}
          <strong>compiler</strong> is the program that turns that C++ code into a program your
          computer can run.
        </p>
        <p>
          Blocks2Cpp uses <strong>g++</strong>, the free C++ compiler of GCC, version 11 or newer.
          You install it once; Blocks2Cpp finds it by itself and never downloads anything for you.
        </p>
      </PageSection>
      {platforms.includes('windows') && <WindowsSteps onOpenLink={onOpenLink} busy={busy} />}
      {platforms.includes('linux') && <LinuxSteps distro={distro} />}
    </>
  );
}

/** A button that opens an install page in the system browser. */
function LinkButton({
  link,
  label,
  onOpenLink,
  busy,
}: {
  link: SetupLink;
  label: string;
  onOpenLink: (link: SetupLink) => void;
  busy: boolean;
}) {
  return (
    <button
      type="button"
      className="button"
      aria-disabled={busy}
      onClick={() => {
        if (!busy) {
          onOpenLink(link);
        }
      }}
    >
      {label}
      <span className="visually-hidden"> (opens in your web browser)</span>
    </button>
  );
}

/** Windows: MSYS2 (recommended), or WinLibs through winget. */
function WindowsSteps({
  onOpenLink,
  busy,
}: {
  onOpenLink: (link: SetupLink) => void;
  busy: boolean;
}) {
  return (
    <PageSection title="Install g++ on Windows" testId="setup-windows">
      <h4 className="setup-option-title">MSYS2 (recommended)</h4>
      <ol className="setup-steps">
        <li>
          <p>Install MSYS2 from its website.</p>
          <LinkButton
            link="msys2Install"
            label="Open msys2.org"
            onOpenLink={onOpenLink}
            busy={busy}
          />
        </li>
        <li>
          <p>
            Open the <strong>MSYS2 UCRT64</strong> shell from the Start menu, and run:
          </p>
          <CopyCommand command={INSTALL_COMMANDS.msys2} />
        </li>
        <li>
          <p>
            Come back here and press <strong>{RESCAN_LABEL}</strong>.
          </p>
        </li>
      </ol>
      <h4 className="setup-option-title">Or: WinLibs</h4>
      <ol className="setup-steps">
        <li>
          <p>Open a terminal (Command Prompt or PowerShell) and run:</p>
          <CopyCommand command={INSTALL_COMMANDS.winlibs} />
          <p className="feature-muted">You can also download WinLibs from its website.</p>
          <LinkButton link="winlibs" label="Open winlibs.com" onOpenLink={onOpenLink} busy={busy} />
        </li>
        <li>
          <p>
            Come back here and press <strong>{RESCAN_LABEL}</strong>.
          </p>
        </li>
      </ol>
    </PageSection>
  );
}

/** Linux: the command of the distribution's package manager, or all three. */
function LinuxSteps({ distro }: { distro: Distro | null }) {
  const managers = linuxCommands(distro);
  return (
    <PageSection title="Install g++ on Linux" testId="setup-linux">
      <ol className="setup-steps">
        <li>
          {managers.length === 1 ? (
            <>
              <p>Open a terminal and run:</p>
              {managers.map((manager) => (
                <CopyCommand key={manager} command={INSTALL_COMMANDS[manager]} />
              ))}
            </>
          ) : (
            <>
              <p>Open a terminal and run the command for your Linux distribution:</p>
              <ul className="setup-alternatives">
                {managers.map((manager) => (
                  <li key={manager}>
                    <p>{PACKAGE_MANAGER_DISTROS[manager]}:</p>
                    <CopyCommand command={INSTALL_COMMANDS[manager]} />
                  </li>
                ))}
              </ul>
            </>
          )}
        </li>
        <li>
          <p>
            Come back here and press <strong>{RESCAN_LABEL}</strong>.
          </p>
        </li>
      </ol>
    </PageSection>
  );
}
