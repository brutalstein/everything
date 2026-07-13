use crate::ModularRuntime;
use anyhow::Result;
use everything_domain::{
    ConnectorActionRequest, ConnectorActionResponse, ConnectorAuditRecord,
    ConnectorConfigureRequest, ConnectorDescriptor, ConnectorProvider, OAuthCallbackRequest,
    OAuthStartRequest, OAuthStartResponse,
};

impl ModularRuntime {
    pub fn list_connectors(&self) -> Result<Vec<ConnectorDescriptor>> {
        self.connector_runtime.list()
    }

    pub fn get_connector(&self, provider: ConnectorProvider) -> Result<ConnectorDescriptor> {
        self.connector_runtime.get(provider)
    }

    pub fn configure_connector(
        &self,
        request: ConnectorConfigureRequest,
    ) -> Result<ConnectorDescriptor> {
        self.connector_runtime.configure(request)
    }

    pub fn disconnect_connector(&self, provider: ConnectorProvider) -> Result<ConnectorDescriptor> {
        self.connector_runtime.disconnect(provider)
    }

    pub fn start_connector_oauth(&self, request: OAuthStartRequest) -> Result<OAuthStartResponse> {
        self.connector_runtime.start_oauth(request)
    }

    pub fn complete_connector_oauth(
        &self,
        request: OAuthCallbackRequest,
    ) -> Result<ConnectorDescriptor> {
        self.connector_runtime.complete_oauth(request)
    }

    pub fn execute_connector_action(
        &self,
        request: ConnectorActionRequest,
    ) -> Result<ConnectorActionResponse> {
        self.connector_runtime.execute(request)
    }

    pub fn connector_audits(&self, limit: usize) -> Result<Vec<ConnectorAuditRecord>> {
        self.connector_runtime.audits(limit)
    }

    pub fn connector_oauth_callback_base(&self) -> &str {
        self.connector_runtime.callback_base()
    }
}
