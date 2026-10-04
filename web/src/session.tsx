import { createContext, useContext } from "react";
import { Instance, User } from "./api";

export type Session = { user: User; instance: Instance | null; host: string };

export const SessionContext = createContext<Session | null>(null);

/** The signed-in user and instance, loaded once by the app shell. */
export function useSession(): Session {
  const session = useContext(SessionContext);
  if (!session) throw new Error("useSession must be used inside the app shell");
  return session;
}
