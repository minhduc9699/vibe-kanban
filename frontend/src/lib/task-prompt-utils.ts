import type { Task } from 'shared/types';

/**
 * Extract plan file path from task description if present.
 * Matches "Phase file: {path}" pattern from plan imports.
 */
export function extractPlanFilePath(description: string | null | undefined): string | null {
  if (!description) return null;
  const lines = description.split('\n');
  const phaseLine = lines.find(line => line.startsWith('Phase file:'));
  if (!phaseLine) return null;
  return phaseLine.replace('Phase file:', '').trim();
}

/**
 * Generate suggested init prompt for task execution.
 * Uses plan path if available, otherwise falls back to title+description.
 */
export function getSuggestedPrompt(task: Task): string {
  const planPath = extractPlanFilePath(task.description);
  if (planPath) {
    return `Implement the plan in ${planPath}`;
  }
  // Fallback to task title + description
  if (task.description?.trim()) {
    return `${task.title}\n\n${task.description}`;
  }
  return task.title;
}
