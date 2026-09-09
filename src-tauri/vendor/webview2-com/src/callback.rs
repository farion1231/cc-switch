use windows::{
    core::{IUnknown, Interface, Ref, BOOL, HRESULT, PCWSTR},
    Win32::System::Com::IStream,
};

use windows_implement::implement;

use crate::{pwstr::string_from_pcwstr, Microsoft::Web::WebView2::Win32::*};

pub trait ClosureArg {
    type Output: Sized;
}

pub trait InvokeArg<'a> {
    type Input: 'a;

    fn convert(input: Self::Input) -> <Self as ClosureArg>::Output
    where
        Self: ClosureArg;
}

impl ClosureArg for HRESULT {
    type Output = windows::core::Result<()>;
}

impl InvokeArg<'_> for HRESULT {
    type Input = Self;

    fn convert(input: HRESULT) -> windows::core::Result<()> {
        input.ok()
    }
}

impl ClosureArg for BOOL {
    type Output = bool;
}

impl InvokeArg<'_> for BOOL {
    type Input = Self;

    fn convert(input: BOOL) -> bool {
        input.as_bool()
    }
}

impl ClosureArg for PCWSTR {
    type Output = String;
}

impl<'a> InvokeArg<'a> for PCWSTR {
    type Input = &'a Self;

    fn convert(input: &PCWSTR) -> String {
        string_from_pcwstr(input)
    }
}

impl<I: Interface> ClosureArg for Option<I> {
    type Output = Self;
}

impl<'a, I: 'a + Interface + Clone> InvokeArg<'a> for Option<I> {
    type Input = Ref<'a, I>;

    fn convert(input: Ref<'a, I>) -> <Self as ClosureArg>::Output {
        input.cloned()
    }
}

/// Generic closure signature for [`completed_callback`].
pub type CompletedClosure<Arg1, Arg2> = Box<
    dyn FnOnce(
        <Arg1 as ClosureArg>::Output,
        <Arg2 as ClosureArg>::Output,
    ) -> ::windows::core::Result<()>,
>;

/// Generic closure signature for [`event_callback`].
pub type EventClosure<Arg1, Arg2> = Box<
    dyn FnMut(
        <Arg1 as ClosureArg>::Output,
        <Arg2 as ClosureArg>::Output,
    ) -> windows::core::Result<()>,
>;

#[event_callback]
pub struct IsDefaultDownloadDialogOpenChangedEventHandler(
    ICoreWebView2IsDefaultDownloadDialogOpenChangedEventHandler,
    Option<ICoreWebView2>,
    Option<IUnknown>,
);

#[event_callback]
pub struct IsDocumentPlayingAudioChangedEventHandler(
    ICoreWebView2IsDocumentPlayingAudioChangedEventHandler,
    Option<ICoreWebView2>,
    Option<IUnknown>,
);

#[event_callback]
pub struct IsMutedChangedEventHandler(
    ICoreWebView2IsMutedChangedEventHandler,
    Option<ICoreWebView2>,
    Option<IUnknown>,
);

#[completed_callback]
pub struct CreateCoreWebView2EnvironmentCompletedHandler(
    ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler,
    HRESULT,
    Option<ICoreWebView2Environment>,
);

#[completed_callback]
pub struct CreateCoreWebView2ControllerCompletedHandler(
    ICoreWebView2CreateCoreWebView2ControllerCompletedHandler,
    HRESULT,
    Option<ICoreWebView2Controller>,
);

#[event_callback]
pub struct NewBrowserVersionAvailableEventHandler(
    ICoreWebView2NewBrowserVersionAvailableEventHandler,
    Option<ICoreWebView2Environment>,
    Option<IUnknown>,
);

#[completed_callback]
pub struct CreateCoreWebView2CompositionControllerCompletedHandler(
    ICoreWebView2CreateCoreWebView2CompositionControllerCompletedHandler,
    HRESULT,
    Option<ICoreWebView2CompositionController>,
);

#[event_callback]
pub struct CursorChangedEventHandler(
    ICoreWebView2CursorChangedEventHandler,
    Option<ICoreWebView2CompositionController>,
    Option<IUnknown>,
);

#[event_callback]
pub struct ZoomFactorChangedEventHandler(
    ICoreWebView2ZoomFactorChangedEventHandler,
    Option<ICoreWebView2Controller>,
    Option<IUnknown>,
);

#[event_callback]
pub struct MoveFocusRequestedEventHandler(
    ICoreWebView2MoveFocusRequestedEventHandler,
    Option<ICoreWebView2Controller>,
    Option<ICoreWebView2MoveFocusRequestedEventArgs>,
);

#[event_callback]
pub struct FocusChangedEventHandler(
    ICoreWebView2FocusChangedEventHandler,
    Option<ICoreWebView2Controller>,
    Option<IUnknown>,
);

#[event_callback]
pub struct AcceleratorKeyPressedEventHandler(
    ICoreWebView2AcceleratorKeyPressedEventHandler,
    Option<ICoreWebView2Controller>,
    Option<ICoreWebView2AcceleratorKeyPressedEventArgs>,
);

#[event_callback]
pub struct ProcessInfosChangedEventHandler(
    ICoreWebView2ProcessInfosChangedEventHandler,
    Option<ICoreWebView2Environment>,
    Option<IUnknown>,
);

#[event_callback]
pub struct RasterizationScaleChangedEventHandler(
    ICoreWebView2RasterizationScaleChangedEventHandler,
    Option<ICoreWebView2Controller>,
    Option<IUnknown>,
);

#[event_callback]
pub struct NavigationStartingEventHandler(
    ICoreWebView2NavigationStartingEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2NavigationStartingEventArgs>,
);

#[event_callback]
pub struct ContentLoadingEventHandler(
    ICoreWebView2ContentLoadingEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2ContentLoadingEventArgs>,
);

#[event_callback]
pub struct SourceChangedEventHandler(
    ICoreWebView2SourceChangedEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2SourceChangedEventArgs>,
);

#[event_callback]
pub struct DOMContentLoadedEventHandler(
    ICoreWebView2DOMContentLoadedEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2DOMContentLoadedEventArgs>,
);

#[event_callback]
pub struct HistoryChangedEventHandler(
    ICoreWebView2HistoryChangedEventHandler,
    Option<ICoreWebView2>,
    Option<IUnknown>,
);

#[event_callback]
pub struct NavigationCompletedEventHandler(
    ICoreWebView2NavigationCompletedEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2NavigationCompletedEventArgs>,
);

#[event_callback]
pub struct ScriptDialogOpeningEventHandler(
    ICoreWebView2ScriptDialogOpeningEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2ScriptDialogOpeningEventArgs>,
);

#[event_callback]
pub struct PermissionRequestedEventHandler(
    ICoreWebView2PermissionRequestedEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2PermissionRequestedEventArgs>,
);

#[completed_callback]
pub struct GetNonDefaultPermissionSettingsCompletedHandler(
    ICoreWebView2GetNonDefaultPermissionSettingsCompletedHandler,
    HRESULT,
    Option<ICoreWebView2PermissionSettingCollectionView>,
);

#[completed_callback]
pub struct SetPermissionStateCompletedHandler(
    ICoreWebView2SetPermissionStateCompletedHandler,
    HRESULT,
);

#[event_callback]
pub struct ProcessFailedEventHandler(
    ICoreWebView2ProcessFailedEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2ProcessFailedEventArgs>,
);

impl ClosureArg for COREWEBVIEW2_PRINT_STATUS {
    type Output = Self;
}

impl InvokeArg<'_> for COREWEBVIEW2_PRINT_STATUS {
    type Input = Self;

    fn convert(input: Self) -> Self {
        input
    }
}

#[completed_callback]
pub struct PrintCompletedHandler(
    ICoreWebView2PrintCompletedHandler,
    HRESULT,
    COREWEBVIEW2_PRINT_STATUS,
);

#[completed_callback]
pub struct PrintToPdfCompletedHandler(ICoreWebView2PrintToPdfCompletedHandler, HRESULT, BOOL);

#[completed_callback]
pub struct PrintToPdfStreamCompletedHandler(
    ICoreWebView2PrintToPdfStreamCompletedHandler,
    HRESULT,
    Option<IStream>,
);

#[completed_callback]
pub struct AddScriptToExecuteOnDocumentCreatedCompletedHandler(
    ICoreWebView2AddScriptToExecuteOnDocumentCreatedCompletedHandler,
    HRESULT,
    PCWSTR,
);

#[completed_callback]
pub struct ExecuteScriptCompletedHandler(
    ICoreWebView2ExecuteScriptCompletedHandler,
    HRESULT,
    PCWSTR,
);

#[completed_callback]
pub struct CapturePreviewCompletedHandler(ICoreWebView2CapturePreviewCompletedHandler, HRESULT);

#[event_callback]
pub struct WebMessageReceivedEventHandler(
    ICoreWebView2WebMessageReceivedEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2WebMessageReceivedEventArgs>,
);

#[completed_callback]
pub struct CallDevToolsProtocolMethodCompletedHandler(
    ICoreWebView2CallDevToolsProtocolMethodCompletedHandler,
    HRESULT,
    PCWSTR,
);

#[event_callback]
pub struct NewWindowRequestedEventHandler(
    ICoreWebView2NewWindowRequestedEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2NewWindowRequestedEventArgs>,
);

#[event_callback]
pub struct DocumentTitleChangedEventHandler(
    ICoreWebView2DocumentTitleChangedEventHandler,
    Option<ICoreWebView2>,
    Option<IUnknown>,
);

#[event_callback]
pub struct ContainsFullScreenElementChangedEventHandler(
    ICoreWebView2ContainsFullScreenElementChangedEventHandler,
    Option<ICoreWebView2>,
    Option<IUnknown>,
);

#[event_callback]
pub struct WebResourceRequestedEventHandler(
    ICoreWebView2WebResourceRequestedEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2WebResourceRequestedEventArgs>,
);

#[event_callback]
pub struct WebResourceResponseReceivedEventHandler(
    ICoreWebView2WebResourceResponseReceivedEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2WebResourceResponseReceivedEventArgs>,
);

#[completed_callback]
pub struct WebResourceResponseViewGetContentCompletedHandler(
    ICoreWebView2WebResourceResponseViewGetContentCompletedHandler,
    HRESULT,
    Option<IStream>,
);

#[event_callback]
pub struct WindowCloseRequestedEventHandler(
    ICoreWebView2WindowCloseRequestedEventHandler,
    Option<ICoreWebView2>,
    Option<IUnknown>,
);

#[event_callback]
pub struct DownloadStartingEventHandler(
    ICoreWebView2DownloadStartingEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2DownloadStartingEventArgs>,
);

#[event_callback]
pub struct BytesReceivedChangedEventHandler(
    ICoreWebView2BytesReceivedChangedEventHandler,
    Option<ICoreWebView2DownloadOperation>,
    Option<IUnknown>,
);

#[event_callback]
pub struct BrowserProcessExitedEventHandler(
    ICoreWebView2BrowserProcessExitedEventHandler,
    Option<ICoreWebView2Environment>,
    Option<ICoreWebView2BrowserProcessExitedEventArgs>,
);

#[event_callback]
pub struct EstimatedEndTimeChangedEventHandler(
    ICoreWebView2EstimatedEndTimeChangedEventHandler,
    Option<ICoreWebView2DownloadOperation>,
    Option<IUnknown>,
);

#[event_callback]
pub struct StateChangedEventHandler(
    ICoreWebView2StateChangedEventHandler,
    Option<ICoreWebView2DownloadOperation>,
    Option<IUnknown>,
);

#[event_callback]
pub struct DevToolsProtocolEventReceivedEventHandler(
    ICoreWebView2DevToolsProtocolEventReceivedEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2DevToolsProtocolEventReceivedEventArgs>,
);

#[event_callback]
pub struct FrameContentLoadingEventHandler(
    ICoreWebView2FrameContentLoadingEventHandler,
    Option<ICoreWebView2Frame>,
    Option<ICoreWebView2ContentLoadingEventArgs>,
);

#[event_callback]
pub struct FrameCreatedEventHandler(
    ICoreWebView2FrameCreatedEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2FrameCreatedEventArgs>,
);

#[event_callback]
pub struct FrameDOMContentLoadedEventHandler(
    ICoreWebView2FrameDOMContentLoadedEventHandler,
    Option<ICoreWebView2Frame>,
    Option<ICoreWebView2DOMContentLoadedEventArgs>,
);

#[event_callback]
pub struct FrameDestroyedEventHandler(
    ICoreWebView2FrameDestroyedEventHandler,
    Option<ICoreWebView2Frame>,
    Option<IUnknown>,
);

#[event_callback]
pub struct FrameNameChangedEventHandler(
    ICoreWebView2FrameNameChangedEventHandler,
    Option<ICoreWebView2Frame>,
    Option<IUnknown>,
);

#[event_callback]
pub struct ClientCertificateRequestedEventHandler(
    ICoreWebView2ClientCertificateRequestedEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2ClientCertificateRequestedEventArgs>,
);

#[completed_callback]
pub struct FrameNavigationCompletedEventHandler(
    ICoreWebView2FrameNavigationCompletedEventHandler,
    Option<ICoreWebView2Frame>,
    Option<ICoreWebView2NavigationCompletedEventArgs>,
);

#[event_callback]
pub struct FrameNavigationStartingEventHandler(
    ICoreWebView2FrameNavigationStartingEventHandler,
    Option<ICoreWebView2Frame>,
    Option<ICoreWebView2NavigationStartingEventArgs>,
);

#[event_callback]
pub struct FrameWebMessageReceivedEventHandler(
    ICoreWebView2FrameWebMessageReceivedEventHandler,
    Option<ICoreWebView2Frame>,
    Option<ICoreWebView2WebMessageReceivedEventArgs>,
);

#[completed_callback]
pub struct GetCookiesCompletedHandler(
    ICoreWebView2GetCookiesCompletedHandler,
    HRESULT,
    Option<ICoreWebView2CookieList>,
);

#[completed_callback]
pub struct TrySuspendCompletedHandler(ICoreWebView2TrySuspendCompletedHandler, HRESULT, BOOL);

#[event_callback]
pub struct BasicAuthenticationRequestedEventHandler(
    ICoreWebView2BasicAuthenticationRequestedEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2BasicAuthenticationRequestedEventArgs>,
);

#[event_callback]
pub struct ContextMenuRequestedEventHandler(
    ICoreWebView2ContextMenuRequestedEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2ContextMenuRequestedEventArgs>,
);

#[event_callback]
pub struct CustomItemSelectedEventHandler(
    ICoreWebView2CustomItemSelectedEventHandler,
    Option<ICoreWebView2ContextMenuItem>,
    Option<IUnknown>,
);

#[event_callback]
pub struct FramePermissionRequestedEventHandler(
    ICoreWebView2FramePermissionRequestedEventHandler,
    Option<ICoreWebView2Frame>,
    Option<ICoreWebView2PermissionRequestedEventArgs2>,
);

#[event_callback]
pub struct StatusBarTextChangedEventHandler(
    ICoreWebView2StatusBarTextChangedEventHandler,
    Option<ICoreWebView2>,
    Option<IUnknown>,
);

#[completed_callback]
pub struct ClearBrowsingDataCompletedHandler(
    ICoreWebView2ClearBrowsingDataCompletedHandler,
    HRESULT,
);

#[completed_callback]
pub struct ClearServerCertificateErrorActionsCompletedHandler(
    ICoreWebView2ClearServerCertificateErrorActionsCompletedHandler,
    HRESULT,
);

#[event_callback]
pub struct ServerCertificateErrorDetectedEventHandler(
    ICoreWebView2ServerCertificateErrorDetectedEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2ServerCertificateErrorDetectedEventArgs>,
);

#[event_callback]
pub struct FaviconChangedEventHandler(
    ICoreWebView2FaviconChangedEventHandler,
    Option<ICoreWebView2>,
    Option<IUnknown>,
);

#[completed_callback]
pub struct GetFaviconCompletedHandler(
    ICoreWebView2GetFaviconCompletedHandler,
    HRESULT,
    Option<IStream>,
);

#[event_callback]
pub struct LaunchingExternalUriSchemeEventHandler(
    ICoreWebView2LaunchingExternalUriSchemeEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2LaunchingExternalUriSchemeEventArgs>,
);

#[completed_callback]
pub struct BrowserExtensionEnableCompletedHandler(
    ICoreWebView2BrowserExtensionEnableCompletedHandler,
    HRESULT,
);

#[completed_callback]
pub struct BrowserExtensionRemoveCompletedHandler(
    ICoreWebView2BrowserExtensionRemoveCompletedHandler,
    HRESULT,
);

#[completed_callback]
pub struct ExecuteScriptWithResultCompletedHandler(
    ICoreWebView2ExecuteScriptWithResultCompletedHandler,
    HRESULT,
    Option<ICoreWebView2ExecuteScriptResult>,
);

#[completed_callback]
pub struct GetProcessExtendedInfosCompletedHandler(
    ICoreWebView2GetProcessExtendedInfosCompletedHandler,
    HRESULT,
    Option<ICoreWebView2ProcessExtendedInfoCollection>,
);

#[completed_callback]
pub struct ProfileAddBrowserExtensionCompletedHandler(
    ICoreWebView2ProfileAddBrowserExtensionCompletedHandler,
    HRESULT,
    Option<ICoreWebView2BrowserExtension>,
);

#[event_callback]
pub struct ProfileDeletedEventHandler(
    ICoreWebView2ProfileDeletedEventHandler,
    Option<ICoreWebView2Profile>,
    Option<IUnknown>,
);

#[completed_callback]
pub struct ProfileGetBrowserExtensionsCompletedHandler(
    ICoreWebView2ProfileGetBrowserExtensionsCompletedHandler,
    HRESULT,
    Option<ICoreWebView2BrowserExtensionList>,
);

#[event_callback]
pub struct NonClientRegionChangedEventHandler(
    ICoreWebView2NonClientRegionChangedEventHandler,
    Option<ICoreWebView2CompositionController>,
    Option<ICoreWebView2NonClientRegionChangedEventArgs>,
);

#[event_callback]
pub struct FrameScreenCaptureStartingEventHandler(
    ICoreWebView2FrameScreenCaptureStartingEventHandler,
    Option<ICoreWebView2Frame>,
    Option<ICoreWebView2ScreenCaptureStartingEventArgs>,
);

#[event_callback]
pub struct NotificationCloseRequestedEventHandler(
    ICoreWebView2NotificationCloseRequestedEventHandler,
    Option<ICoreWebView2Notification>,
    Option<IUnknown>,
);

#[event_callback]
pub struct NotificationReceivedEventHandler(
    ICoreWebView2NotificationReceivedEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2NotificationReceivedEventArgs>,
);

#[event_callback]
pub struct SaveAsUIShowingEventHandler(
    ICoreWebView2SaveAsUIShowingEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2SaveAsUIShowingEventArgs>,
);

#[event_callback]
pub struct SaveFileSecurityCheckStartingEventHandler(
    ICoreWebView2SaveFileSecurityCheckStartingEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2SaveFileSecurityCheckStartingEventArgs>,
);

#[event_callback]
pub struct ScreenCaptureStartingEventHandler(
    ICoreWebView2ScreenCaptureStartingEventHandler,
    Option<ICoreWebView2>,
    Option<ICoreWebView2ScreenCaptureStartingEventArgs>,
);

#[event_callback]
pub struct FrameChildFrameCreatedEventHandler(
    ICoreWebView2FrameChildFrameCreatedEventHandler,
    Option<ICoreWebView2Frame>,
    Option<ICoreWebView2FrameCreatedEventArgs>,
);

#[event_callback]
pub struct FindActiveMatchIndexChangedEventHandler(
    ICoreWebView2FindActiveMatchIndexChangedEventHandler,
    Option<ICoreWebView2Find>,
    Option<IUnknown>,
);

#[event_callback]
pub struct FindMatchCountChangedEventHandler(
    ICoreWebView2FindMatchCountChangedEventHandler,
    Option<ICoreWebView2Find>,
    Option<IUnknown>,
);

#[completed_callback]
pub struct FindStartCompletedHandler(ICoreWebView2FindStartCompletedHandler, HRESULT);

impl ClosureArg for COREWEBVIEW2_SAVE_AS_UI_RESULT {
    type Output = Self;
}

impl InvokeArg<'_> for COREWEBVIEW2_SAVE_AS_UI_RESULT {
    type Input = Self;

    fn convert(input: Self) -> Self {
        input
    }
}

#[completed_callback]
pub struct ShowSaveAsUICompletedHandler(
    ICoreWebView2ShowSaveAsUICompletedHandler,
    HRESULT,
    COREWEBVIEW2_SAVE_AS_UI_RESULT,
);

#[cfg(test)]
mod test {
    use std::collections::BTreeSet;

    use regex::Regex;

    use webview2_com_sys::declared_interfaces;

    #[test]
    fn all_implemented() {
        let contents = include_str!("callback.rs");
        let pattern = Regex::new(r#"(ICoreWebView2[A-Za-z0-9]+Handler)"#).expect("valid regex");
        let implemented: BTreeSet<&str> = contents
            .lines()
            .filter_map(|line| pattern.captures(line))
            .filter_map(|captures| captures.get(1))
            .map(|match_1| match_1.as_str())
            .collect();
        let all_declared_callbacks = declared_interfaces::all_declared_callbacks();
        let missing: Vec<_> = all_declared_callbacks
            .iter()
            .filter_map(|name| {
                if implemented.contains(name) {
                    None
                } else {
                    Some(name.to_string())
                }
            })
            .collect();
        let extra: Vec<_> = implemented
            .iter()
            .filter_map(|name| {
                if all_declared_callbacks.contains(name) {
                    None
                } else {
                    Some(name.to_string())
                }
            })
            .collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "missing: {missing:?}\nextra: {extra:?}"
        );
    }
}
