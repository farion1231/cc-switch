import { createContext, useContext } from "react";

// This affects form editing only. Native save validation is authoritative.
export const CredentialEditProtectionContext = createContext(false);
export const useCredentialEditProtection = () =>
  useContext(CredentialEditProtectionContext);
