/**
 * What the docks show. Milestone M2's waves 3 and 4 connect the real panels here (the C++ view and
 * Problems in wave 3, the console and the build output in wave 4); until then each slot holds a
 * short note saying what will appear.
 */
import type { ReactNode } from 'react';

/** The content of each dock slot. */
export interface DockPanelContents {
  /** The right dock's C++ tab. */
  code: ReactNode;
  /** The bottom dock's Problems tab. */
  problems: ReactNode;
  /** The bottom dock's Console tab. */
  console: ReactNode;
  /** The bottom dock's Build output tab. */
  buildOutput: ReactNode;
}

/** A note in an empty panel. */
function EmptyPanel({ children }: { children: ReactNode }) {
  return <p className="empty-panel">{children}</p>;
}

/**
 * The dock slots' content. It is called during the layout's render, so it may use hooks, and it
 * must call the same hooks every time.
 */
export function DockPanels(): DockPanelContents {
  return {
    code: <EmptyPanel>The C++ for your blocks will appear here.</EmptyPanel>,
    problems: <EmptyPanel>Problems in your blocks will be listed here.</EmptyPanel>,
    console: <EmptyPanel>Your program&apos;s output will appear here.</EmptyPanel>,
    buildOutput: <EmptyPanel>The compiler&apos;s messages will appear here.</EmptyPanel>,
  };
}
