import type { Feature } from '../../app/features';
import { ProjectSettingsPage } from './ProjectSettingsPage';

/**
 * The project settings feature (docs/spec/04-user-interface.md §4.1): the `projectSettings` page
 * and the `project.settings` command that opens it (≡ → *Project settings…*), while a project is
 * open.
 */
export const projectSettingsFeature: Feature = (ctx) => {
  function ProjectSettingsScreen() {
    return <ProjectSettingsPage ctx={ctx} />;
  }
  const removers = [
    ctx.screens.registerScreen('projectSettings', ProjectSettingsScreen),
    ctx.commands.registerCommand('project.settings', () => {
      if (ctx.store.getState().project !== null) {
        ctx.store.getState().actions.setUi({ screen: 'projectSettings' });
      }
    }),
  ];
  return () => {
    for (const remove of removers.reverse()) {
      remove();
    }
  };
};
