import type { ReactNode } from 'react';

interface PanelPlaceholderProps {
  /** The panel's heading. */
  title: string;
  /** What the panel will show once it is implemented. */
  children: ReactNode;
}

/** A labelled, empty panel for a part of the window that later milestones fill in. */
export function PanelPlaceholder({ title, children }: PanelPlaceholderProps) {
  return (
    <div className="panel-placeholder">
      <h2 className="panel-title">{title}</h2>
      <p className="placeholder-text">{children}</p>
    </div>
  );
}
