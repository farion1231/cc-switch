import { post } from "../web-client";

export const webAuthApi = {
  async login(token: string): Promise<{ token: string }> {
    return post("/auth/login", { token });
  },
  async logout(): Promise<void> {
    return post("/auth/logout", {});
  },
};
