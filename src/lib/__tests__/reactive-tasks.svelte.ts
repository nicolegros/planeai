/** A reactive stand-in for the task store, so tests can change tasks the way the store does. */
let tasks = $state<object[]>([]);

export function getAllTasks(): object[] {
  return tasks;
}

export function setTasks(next: object[]): void {
  tasks = next;
}
