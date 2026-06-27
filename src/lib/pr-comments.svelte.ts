import { pr } from "./api";
import { createPoller } from "./poller.svelte";

const poller = createPoller<number>({ fetch: pr.getPrComments });

export const getCommentCount = (sessionId: string): number => poller.get(sessionId) ?? 0;
export const refreshPrComments = poller.refresh;
export const startPolling = poller.startPolling;
export const updateSessions = poller.updateSessions;
