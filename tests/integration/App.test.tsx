import { Suspense, type ComponentType } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  render,
  waitFor,
  fireEvent,
  type RenderResult,
} from "@testing-library/react";
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { http, HttpResponse } from "msw";
import { providersApi } from "@/lib/api/providers";
import {
  resetProviderState,
  setCurrentProviderId,
  setLiveProviderIds,
  setProviders,
} from "../msw/state";
import { emitTauriEvent } from "../msw/tauriMocks";
import { server } from "../msw/server";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const skillsPanelMocks = vi.hoisted(() => ({
  checkUpdates: vi.fn(),
  openDiscovery: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("@/components/providers/ProviderList", () => ({
  ProviderList: ({
    providers,
    currentProviderId,
    onSwitch,
    onEdit,
    onDuplicate,
    onConfigureUsage,
    onOpenWebsite,
    onCreate,
    onDelete,
    onRemoveFromConfig,
  }: any) => (
    <div>
      <div data-testid="provider-list">{JSON.stringify(providers)}</div>
      <div data-testid="current-provider">{currentProviderId}</div>
      <button onClick={() => onSwitch(providers[currentProviderId])}>
        switch
      </button>
      <button onClick={() => onEdit(providers[currentProviderId])}>edit</button>
      <button onClick={() => onDuplicate(providers[currentProviderId])}>
        duplicate
      </button>
      <button onClick={() => onConfigureUsage(providers[currentProviderId])}>
        usage
      </button>
      <button onClick={() => onOpenWebsite("https://example.com")}>
        open-website
      </button>
      <button onClick={() => onDelete(Object.values(providers)[0])}>
        delete
      </button>
      <button onClick={() => onRemoveFromConfig?.(Object.values(providers)[0])}>
        remove
      </button>
      <button onClick={() => onCreate?.()}>create</button>
    </div>
  ),
}));

vi.mock("@/components/providers/AddProviderDialog", () => ({
  AddProviderDialog: ({ open, onOpenChange, onSubmit, appId }: any) =>
    open ? (
      <div data-testid="add-provider-dialog">
        <button
          onClick={() =>
            onSubmit({
              name: `New ${appId} Provider`,
              settingsConfig: {},
              category: "custom",
              sortIndex: 99,
            })
          }
        >
          confirm-add
        </button>
        <button onClick={() => onOpenChange(false)}>close-add</button>
      </div>
    ) : null,
}));

vi.mock("@/components/providers/EditProviderDialog", () => ({
  EditProviderDialog: ({ open, provider, onSubmit, onOpenChange }: any) =>
    open ? (
      <div data-testid="edit-provider-dialog">
        <button
          onClick={() =>
            onSubmit({
              provider: {
                ...provider,
                name: `${provider.name}-edited`,
              },
              originalId: provider.id,
            })
          }
        >
          confirm-edit
        </button>
        <button onClick={() => onOpenChange(false)}>close-edit</button>
      </div>
    ) : null,
}));

vi.mock("@/components/UsageScriptModal", () => ({
  default: ({ isOpen, provider, onSave, onClose }: any) =>
    isOpen ? (
      <div data-testid="usage-modal">
        <span data-testid="usage-provider">{provider?.id}</span>
        <button onClick={() => onSave("script-code")}>save-script</button>
        <button onClick={() => onClose()}>close-usage</button>
      </div>
    ) : null,
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: ({ isOpen, message, onConfirm, onCancel }: any) =>
    isOpen ? (
      <div data-testid="confirm-dialog">
        <div data-testid="confirm-message">{message}</div>
        <button onClick={() => onConfirm()}>confirm-delete</button>
        <button onClick={() => onCancel()}>cancel-delete</button>
      </div>
    ) : null,
}));

vi.mock("@/components/AppSwitcher", () => ({
  AppSwitcher: ({ activeApp, onSwitch }: any) => (
    <div data-testid="app-switcher">
      <span>{activeApp}</span>
      <button onClick={() => onSwitch("claude")}>switch-claude</button>
      <button onClick={() => onSwitch("codex")}>switch-codex</button>
      <button onClick={() => onSwitch("openclaw")}>switch-openclaw</button>
    </div>
  ),
}));

vi.mock("@/components/skills/UnifiedSkillsPanel", async () => {
  const React = await import("react");
  const MockUnifiedSkillsPanel = React.forwardRef(
    ({ onCheckUpdatesStateChange }: any, ref) => {
      React.useEffect(() => {
        onCheckUpdatesStateChange?.({ isChecking: false, hasSkills: true });
        return () =>
          onCheckUpdatesStateChange?.({
            isChecking: false,
            hasSkills: false,
          });
      }, [onCheckUpdatesStateChange]);
      React.useImperativeHandle(ref, () => ({
        openDiscovery: skillsPanelMocks.openDiscovery,
        openImport: vi.fn(),
        openInstallFromZip: vi.fn(),
        openRestoreFromBackup: vi.fn(),
        checkUpdates: skillsPanelMocks.checkUpdates,
      }));
      return <div data-testid="unified-skills-panel" />;
    },
  );
  MockUnifiedSkillsPanel.displayName = "MockUnifiedSkillsPanel";
  return { default: MockUnifiedSkillsPanel };
});

vi.mock("@/components/UpdateBadge", () => ({
  UpdateBadge: ({ onClick }: any) => (
    <button onClick={onClick}>update-badge</button>
  ),
}));

vi.mock("@/components/mcp/McpPanel", () => ({
  default: ({ open, onOpenChange }: any) =>
    open ? (
      <div data-testid="mcp-panel">
        <button onClick={() => onOpenChange(false)}>close-mcp</button>
      </div>
    ) : (
      <button onClick={() => onOpenChange(true)}>open-mcp</button>
    ),
}));

const mountedApps = new Set<{
  view: RenderResult;
  queryClient: QueryClient;
}>();

const APP_QUERY_TIMEOUT = 15_000;
const APP_TEST_TIMEOUT = 30_000;

const renderApp = (AppComponent: ComponentType): RenderResult => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
        refetchOnWindowFocus: false,
        gcTime: 0,
      },
      mutations: {
        retry: false,
      },
    },
  });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <Suspense fallback={<div data-testid="loading">loading</div>}>
        <AppComponent />
      </Suspense>
    </QueryClientProvider>,
  );
  mountedApps.add({ view, queryClient });
  return view;
};

const waitForProviderList = async (view: RenderResult, expected: string) => {
  await waitFor(
    () =>
      expect(view.getByTestId("provider-list").textContent).toContain(expected),
    { timeout: APP_QUERY_TIMEOUT },
  );
};

afterEach(async () => {
  const apps = [...mountedApps];
  mountedApps.clear();

  for (const { view, queryClient } of apps) {
    view.unmount();
    await queryClient.cancelQueries();
    queryClient.clear();
  }
});

describe("App integration with MSW", () => {
  beforeEach(() => {
    resetProviderState();
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    skillsPanelMocks.checkUpdates.mockReset();
    skillsPanelMocks.openDiscovery.mockReset();
    localStorage.removeItem("cc-switch-last-view");
    localStorage.removeItem("cc-switch-last-app");
  });

  it(
    "covers basic provider flows via real hooks",
    async () => {
      const { default: App } = await import("@/App");
      const view = renderApp(App);

      await waitForProviderList(view, "claude-1");

      fireEvent.click(view.getByText("switch-codex"));
      await waitForProviderList(view, "codex-1");

      fireEvent.click(view.getByText("usage"));
      expect(view.getByTestId("usage-modal")).toBeInTheDocument();
      fireEvent.click(view.getByText("save-script"));
      fireEvent.click(view.getByText("close-usage"));

      fireEvent.click(view.getByText("create"));
      expect(view.getByTestId("add-provider-dialog")).toBeInTheDocument();
      fireEvent.click(view.getByText("confirm-add"));
      await waitForProviderList(view, "New codex Provider");

      fireEvent.click(view.getByText("edit"));
      expect(view.getByTestId("edit-provider-dialog")).toBeInTheDocument();
      fireEvent.click(view.getByText("confirm-edit"));
      await waitForProviderList(view, "-edited");

      fireEvent.click(view.getByText("switch"));
      fireEvent.click(view.getByText("duplicate"));
      await waitForProviderList(view, "copy");

      fireEvent.click(view.getByText("open-website"));

      emitTauriEvent("provider-switched", {
        appType: "codex",
        providerId: "codex-2",
      });

      expect(toastErrorMock).not.toHaveBeenCalled();
      expect(toastSuccessMock).toHaveBeenCalled();
    },
    APP_TEST_TIMEOUT,
  );

  it(
    "shows toast when auto sync fails in background",
    async () => {
      const { default: App } = await import("@/App");
      const view = renderApp(App);

      await waitForProviderList(view, "claude-1");

      expect(() => {
        emitTauriEvent("webdav-sync-status-updated", null);
      }).not.toThrow();
      expect(toastErrorMock).not.toHaveBeenCalled();

      emitTauriEvent("webdav-sync-status-updated", {
        source: "auto",
        status: "error",
        error: "network timeout",
      });

      await waitFor(() => {
        expect(toastErrorMock).toHaveBeenCalled();
      });

      toastErrorMock.mockReset();
      expect(() => {
        emitTauriEvent("s3-sync-status-updated", null);
      }).not.toThrow();
      expect(toastErrorMock).not.toHaveBeenCalled();

      emitTauriEvent("s3-sync-status-updated", {
        source: "auto",
        status: "error",
        error: "s3 timeout",
      });

      await waitFor(() => {
        expect(toastErrorMock).toHaveBeenCalled();
      });
    },
    APP_TEST_TIMEOUT,
  );

  it(
    "duplicates openclaw providers with a generated key that avoids live-only ids",
    async () => {
      setProviders("openclaw", {
        deepseek: {
          id: "deepseek",
          name: "DeepSeek",
          settingsConfig: {
            baseUrl: "https://api.deepseek.com",
            apiKey: "test-key",
            api: "openai-completions",
            models: [],
          },
          category: "custom",
          sortIndex: 0,
          createdAt: Date.now(),
        },
      });
      setCurrentProviderId("openclaw", "deepseek");
      setLiveProviderIds("openclaw", ["deepseek-copy"]);

      const { default: App } = await import("@/App");
      const view = renderApp(App);

      fireEvent.click(view.getByText("switch-openclaw"));

      await waitForProviderList(view, "deepseek");

      fireEvent.click(view.getByText("duplicate"));

      await waitFor(() => {
        const providerList = view.getByTestId("provider-list").textContent;
        expect(providerList).toContain("deepseek-copy-2");
        expect(providerList).toContain("DeepSeek copy");
      });

      expect(toastErrorMock).not.toHaveBeenCalledWith(
        expect.stringContaining("Provider key is required for openclaw"),
      );
    },
    APP_TEST_TIMEOUT,
  );

  it(
    "warns without blocking when removing Pi's global default provider",
    async () => {
      localStorage.setItem("cc-switch-last-app", "pi");
      setProviders("pi", {
        custom: {
          id: "custom",
          name: "Custom Pi",
          settingsConfig: {
            baseUrl: "https://api.example.com/v1",
            apiKey: "test-key",
            api: "openai-completions",
            models: [{ id: "model-a" }],
          },
          category: "custom",
          sortIndex: 0,
          createdAt: Date.now(),
        },
      });
      server.use(
        http.post("http://tauri.local/get_pi_current_state", () =>
          HttpResponse.json({
            enabledProviderIds: ["custom"],
            defaultProviderId: "custom",
          }),
        ),
      );

      const { default: App } = await import("@/App");
      const view = renderApp(App);

      await waitForProviderList(view, "Custom Pi");
      fireEvent.click(view.getByText("remove"));

      expect(view.getByTestId("confirm-message")).toHaveTextContent(
        "confirm.piDefaultProviderWarning",
      );
      fireEvent.click(view.getByText("confirm-delete"));
      await waitFor(() =>
        expect(view.queryByTestId("confirm-dialog")).not.toBeInTheDocument(),
      );
    },
    APP_TEST_TIMEOUT,
  );

  it(
    "shows toast when duplicate cannot load live provider ids",
    async () => {
      setProviders("openclaw", {
        deepseek: {
          id: "deepseek",
          name: "DeepSeek",
          settingsConfig: {
            baseUrl: "https://api.deepseek.com",
            apiKey: "test-key",
            api: "openai-completions",
            models: [],
          },
          category: "custom",
          sortIndex: 0,
          createdAt: Date.now(),
        },
      });
      setCurrentProviderId("openclaw", "deepseek");

      const liveIdsSpy = vi
        .spyOn(providersApi, "getOpenClawLiveProviderIds")
        .mockRejectedValueOnce(new Error("broken config"));

      const { default: App } = await import("@/App");
      const view = renderApp(App);

      fireEvent.click(view.getByText("switch-openclaw"));

      await waitForProviderList(view, "deepseek");

      fireEvent.click(view.getByText("duplicate"));

      await waitFor(() => {
        expect(toastErrorMock).toHaveBeenCalledWith(
          expect.stringContaining("读取配置中的供应商标识失败"),
        );
      });

      expect(view.getByTestId("provider-list").textContent).not.toContain(
        "deepseek-copy",
      );

      liveIdsSpy.mockRestore();
    },
    APP_TEST_TIMEOUT,
  );

  it(
    "hosts the Skills check-update action in the App toolbar",
    async () => {
      localStorage.setItem("cc-switch-last-view", "skills");
      const { default: App } = await import("@/App");
      const view = renderApp(App);

      expect(
        await view.findByTestId("unified-skills-panel"),
      ).toBeInTheDocument();
      const checkUpdatesButton = await view.findByRole("button", {
        name: "skills.checkUpdates",
      });
      await waitFor(() => expect(checkUpdatesButton).toBeEnabled());

      fireEvent.click(checkUpdatesButton);
      expect(skillsPanelMocks.checkUpdates).toHaveBeenCalledTimes(1);
    },
    APP_TEST_TIMEOUT,
  );

  it(
    "routes the Skills discover toolbar action through the panel guard",
    async () => {
      localStorage.setItem("cc-switch-last-view", "skills");
      const { default: App } = await import("@/App");
      const view = renderApp(App);

      expect(
        await view.findByTestId("unified-skills-panel"),
      ).toBeInTheDocument();
      fireEvent.click(
        await view.findByRole("button", {
          name: "skills.discover",
        }),
      );

      expect(skillsPanelMocks.openDiscovery).toHaveBeenCalledTimes(1);
      expect(view.getByTestId("unified-skills-panel")).toBeInTheDocument();
    },
    APP_TEST_TIMEOUT,
  );
});
