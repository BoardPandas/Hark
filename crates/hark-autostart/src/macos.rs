use objc2_service_management::{SMAppService, SMAppServiceStatus};

use crate::Error;

pub(super) fn reconcile(enabled: bool) -> Result<(), Error> {
    // Hark's bundle requires macOS 14.2; SMAppService is available since 13.
    // ServiceManagement retains the main bundle and owns the login item.
    unsafe {
        let service = SMAppService::mainAppService();
        let status = service.status();
        if enabled {
            match status {
                SMAppServiceStatus::Enabled => Ok(()),
                SMAppServiceStatus::RequiresApproval => Err(Error::LoginItem(
                    "Allow Hark in System Settings → General → Login Items".into(),
                )),
                _ => service
                    .registerAndReturnError()
                    .map_err(|e| Error::LoginItem(e.to_string())),
            }
        } else if matches!(
            status,
            SMAppServiceStatus::NotRegistered | SMAppServiceStatus::NotFound
        ) {
            Ok(())
        } else {
            service
                .unregisterAndReturnError()
                .map_err(|e| Error::LoginItem(e.to_string()))
        }
    }
}

pub(super) fn is_enabled() -> Result<bool, Error> {
    // Read the OS approval state, not a cached preference.
    Ok(unsafe { SMAppService::mainAppService().status() == SMAppServiceStatus::Enabled })
}
