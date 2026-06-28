import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { LoginPage } from "@/components/auth/LoginPage";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

const setAuthTokenMock = vi.fn();
const loginMock = vi.fn();

vi.mock("@tauri-icons/icon.png", () => ({
  default: "/mocked-icon.png",
}));

vi.mock("@/lib/api/web-client", () => ({
  setAuthToken: (...args: unknown[]) => setAuthTokenMock(...args),
}));

vi.mock("@/lib/api", () => ({
  authApi: {
    login: (...args: unknown[]) => loginMock(...args),
  },
}));

const renderLoginPage = (props: { onLogin?: () => void } = {}) => {
  return render(<LoginPage onLogin={props.onLogin ?? vi.fn()} />);
};

describe("LoginPage Component", () => {
  beforeEach(() => {
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    setAuthTokenMock.mockReset();
    loginMock.mockReset();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("renders the login form with token input and submit button", () => {
    renderLoginPage();

    expect(screen.getByLabelText("Auth Token")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Sign In" }),
    ).toBeInTheDocument();
  });

  it("does not render the reveal token button", () => {
    renderLoginPage();

    expect(screen.queryByText("Reveal Token")).not.toBeInTheDocument();
  });

  it("shows error toast when submitting empty token", async () => {
    renderLoginPage();

    fireEvent.click(screen.getByRole("button", { name: "Sign In" }));

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalled();
    });
  });

  it("calls login, stores JWT, and invokes onLogin on success", async () => {
    loginMock.mockResolvedValue("jwt-token-from-server");
    const onLogin = vi.fn();

    renderLoginPage({ onLogin });

    const input = screen.getByPlaceholderText("Paste your auth token here");
    fireEvent.change(input, { target: { value: "user-auth-token" } });

    fireEvent.click(screen.getByRole("button", { name: "Sign In" }));

    await waitFor(() => {
      expect(loginMock).toHaveBeenCalledWith("user-auth-token");
      expect(setAuthTokenMock).toHaveBeenCalledWith("jwt-token-from-server");
      expect(toastSuccessMock).toHaveBeenCalled();
      expect(onLogin).toHaveBeenCalledTimes(1);
    });
  });

  it("shows error toast when login fails", async () => {
    loginMock.mockRejectedValue(new Error("Invalid auth token"));

    renderLoginPage();

    const input = screen.getByPlaceholderText("Paste your auth token here");
    fireEvent.change(input, { target: { value: "wrong-token" } });

    fireEvent.click(screen.getByRole("button", { name: "Sign In" }));

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalled();
    });
    expect(setAuthTokenMock).not.toHaveBeenCalled();
  });

  it("disables the input and submit button during submit", async () => {
    let resolveLogin: (value: string) => void;
    const loginPromise = new Promise<string>(
      (resolve) => (resolveLogin = resolve),
    );
    loginMock.mockReturnValue(loginPromise);

    renderLoginPage();

    const input = screen.getByPlaceholderText(
      "Paste your auth token here",
    ) as HTMLInputElement;
    const submitButton = screen.getByRole("button", { name: "Sign In" });

    fireEvent.change(input, { target: { value: "my-token" } });
    fireEvent.click(submitButton);

    await waitFor(() => {
      expect(input).toBeDisabled();
      expect(submitButton).toBeDisabled();
    });

    resolveLogin!("jwt");
    await waitFor(() => expect(input).not.toBeDisabled());
  });
});
