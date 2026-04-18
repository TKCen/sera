/**
 * Heartbeat — sends periodic heartbeats to Core to prove the container is alive.
 */

import axios, { AxiosError } from 'axios';
import { log } from './logger.js';

const DEFAULT_INTERVAL_MS = 10_000; // 10 seconds

/**
 * Start sending heartbeats to Core.
 * Returns a cleanup function to stop the heartbeat interval.
 */
export function startHeartbeat(
  coreUrl: string,
  agentId: string,
  identityToken: string,
  intervalMs: number = DEFAULT_INTERVAL_MS,
): () => void {
  let running = true;

  const sendBeat = async () => {
    if (!running) return;

    try {
      await axios.post(
        `${coreUrl}/api/agents/${agentId}/heartbeat`,
        {},
        {
          headers: {
            'Authorization': `Bearer ${identityToken}`,
            'Content-Type': 'application/json',
          },
          timeout: 5000,
        },
      );
      log('debug', 'Heartbeat sent');
    } catch (err) {
      const msg = err instanceof AxiosError ? err.message : String(err);
      log('warn', `Heartbeat failed: ${msg}`);
      // Don't crash — heartbeat failures are non-fatal
    }
  };

  // Send first heartbeat immediately
  sendBeat();

  const timer = setInterval(sendBeat, intervalMs);

  return () => {
    running = false;
    clearInterval(timer);
    log('info', 'Heartbeat stopped');
  };
}
