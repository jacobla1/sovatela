// Project membership: which project a chat belongs to, and whether that
// project still exists.
//
// Deleting a project used to leave every chat in it carrying an id that
// resolved to nothing. Opening one of those chats put the sidebar into a
// project with no name and no instructions, and every new chat started from
// there silently joined the thing that had been deleted.
//
// 1.5.5 added an effect that reconciled the selection whenever the project
// *list* changed. That covered the wrong moment: the stale id arrives when a
// conversation is opened, which is not a change to the list, so the effect did
// not run and the id survived. The check belongs where the id is assigned.

/**
 * Return `id` if it names a project that exists, and `null` if it does not.
 *
 * `loaded` says whether the project list has arrived yet. Before it has, an
 * empty list means "not known" rather than "no projects", and an id is kept —
 * discarding it there would drop a real membership during startup, which is
 * the same defect from the other direction. The effect that watches the list
 * catches up once it arrives.
 *
 * @param {string|null|undefined} id
 * @param {Array<{id: string}>} projects
 * @param {boolean} loaded
 * @returns {string|null}
 */
export function knownProjectId(id, projects, loaded) {
  if (!id) return null;
  if (!loaded) return id;
  return projects.some((p) => p.id === id) ? id : null;
}
