// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { PostProcessingKey as SettingKey } from './config.ts';
import type { PostProcessingProfile as Profile } from './profile.ts';

import { resolvePostProcessingProfile, validatePostProcessingProfile } from './profile.ts';

export type { Profile };
export type { SettingKey };

/** Own authored edit history independently of the renderer, transport, and gameplay.
 * All snapshots returned to callers are copied. Transient preview modes never enter history.
 */
export class PostProcessingSession {
  declare savedProfile: Profile;
  declare draftProfile: Profile;
  declare revision: string;
  declare saving: boolean;
  declare conflict: { profile: Profile; revision: string; } | null;
  declare undoHistory: Profile[];
  declare redoHistory: Profile[];

  /** @param profile @param revision Opaque disk revision. */
  constructor(profile: unknown, revision: string) {
    this.savedProfile = validatePostProcessingProfile(profile);
    this.draftProfile = validatePostProcessingProfile(profile);
    this.revision = revision;
    this.saving = false;

    this.conflict = null;

    this.undoHistory = [];

    this.redoHistory = [];
  }

  get draft() {
    return validatePostProcessingProfile(this.draftProfile);
  }
  get saved() {
    return validatePostProcessingProfile(this.savedProfile);
  }
  get config() {
    return resolvePostProcessingProfile(this.draftProfile);
  }
  get dirty() {
    return !equalProfiles(this.draftProfile, this.savedProfile);
  }
  get canUndo() {
    return this.undoHistory.length > 0;
  }
  get canRedo() {
    return this.redoHistory.length > 0;
  }

  /** Replace the authored draft atomically; callers may import another game's profile.
   * @param profile
   */
  replace(profile: unknown) {
    const validated = validatePostProcessingProfile(profile);
    if(equalProfiles(validated, this.draftProfile)) {
      return;
    }
    this.undoHistory.push(this.draft);
    this.undoHistory = this.undoHistory.slice(-100);
    this.redoHistory = [];
    this.draftProfile = validated;
  }

  /** @param key @param value */
  setValue(key: SettingKey, value: number | string) {
    this.replace({ ...this.draftProfile, overrides: { ...this.draftProfile.overrides, [key]: value } });
  }

  /** Remove an override so later preset changes can propagate. @param key */
  resetValue(key: SettingKey) {
    const profile = this.draft;
    delete profile.overrides[key];
    this.replace(profile);
  }

  /** Select a starting point, clearing game overrides as one undoable edit. @param preset */
  selectPreset(preset: string) {
    this.replace({ version: 1, preset, overrides: {} });
  }

  undo() {
    const previous = this.undoHistory.pop();
    if(previous) {
      this.redoHistory.push(this.draft);
      this.draftProfile = previous;
    }
  }

  redo() {
    const next = this.redoHistory.pop();
    if(next) {
      this.undoHistory.push(this.draft);
      this.draftProfile = next;
    }
  }

  /** Restore the saved snapshot as an undoable draft edit. */
  revert() {
    this.replace(this.savedProfile);
  }

  /** Accept clean file edits immediately; preserve dirty drafts pending a choice.
   * @param profile @param revision
   */
  receiveExternal(profile: unknown, revision: string) {
    const validated = validatePostProcessingProfile(profile);
    if(revision === this.revision) {
      this.conflict = null;
      return;
    }
    if((this.dirty || this.saving) && !equalProfiles(validated, this.draftProfile)) {
      this.conflict = { profile: validated, revision };
      return;
    }
    this.adoptDisk(validated, revision);
  }

  /** Explicitly retain the draft against the latest disk revision. */
  keepLocal() {
    if(!this.conflict) {
      return;
    }
    this.savedProfile = this.conflict.profile;
    this.revision = this.conflict.revision;
    this.conflict = null;
  }

  /** Resolve a pending disk conflict by replacing the draft and clearing edit history. */
  useDisk() {
    if(this.conflict) {
      this.adoptDisk(this.conflict.profile, this.conflict.revision);
    }
  }

  /** Capture the submitted draft while protecting newer edits from watcher echoes. */
  beginSave() {
    this.saving = true;
    return this.draft;
  }

  /** Release pending-save protection after a failed or cancelled request. */
  endSave() {
    this.saving = false;
  }

  /** Acknowledge exactly the submitted snapshot, retaining newer local edits.
   * @param submitted @param revision
   */
  markSaved(submitted: unknown, revision: string) {
    this.saving = false;
    this.savedProfile = validatePostProcessingProfile(submitted);
    this.revision = revision;
    if(this.conflict?.revision === revision) {
      this.conflict = null;
    }
  }

  /** @private @param profile @param revision */
  private adoptDisk(profile: Profile, revision: string) {
    this.savedProfile = validatePostProcessingProfile(profile);
    this.draftProfile = validatePostProcessingProfile(profile);
    this.revision = revision;
    this.conflict = null;
    this.undoHistory = [];
    this.redoHistory = [];
  }
}

/** @param left @param right */
function equalProfiles(left: Profile, right: Profile) {
  return JSON.stringify(left) === JSON.stringify(right);
}
