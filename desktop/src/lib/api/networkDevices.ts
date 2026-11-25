/** The PCs of the local network: see `local_server/network_devices.rs`. */
import { api } from "./client";

export interface NetworkDevice {
  device_id: string;
  name: string;
  first_seen: string;
  last_seen: string;
  /** Set once an administrator has removed it. */
  removed_at: string | null;
  this_pc: boolean;
}

export function listNetworkDevices(): Promise<NetworkDevice[]> {
  return api.get<NetworkDevice[]>("/api/network/devices");
}

/** Shut a PC out: the network gets a new key and the administrator a new password. */
export function removeNetworkDevice(
  id: string,
  p: { password: string; new_password: string; confirm: string },
): Promise<void> {
  return api.post(`/api/network/devices/${id}/remove`, p, { timeoutMs: 60_000 });
}

/** On a PC left with an old key: get the current one with an administrator's password. */
export function reconnectToNetwork(p: { username: string; password: string }): Promise<void> {
  return api.post("/api/auth/local/reconnect", p, { auth: false, timeoutMs: 60_000 });
}
