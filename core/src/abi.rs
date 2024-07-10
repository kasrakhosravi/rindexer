use crate::database::postgres::{
    solidity_type_to_db_type, solidity_type_to_ethereum_sql_type_wrapper,
};
use crate::generator::event_callback_registry::IndexingContractSetup;
use crate::helpers::{camel_to_snake, get_full_path};
use crate::manifest::yaml::Contract;
use crate::EthereumSqlTypeWrapper;
use ethers::utils::keccak256;
use serde::{Deserialize, Serialize};
use std::fs;
use std::iter::Map;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ABIInput {
    pub indexed: Option<bool>,
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub components: Option<Vec<ABIInput>>,
}

#[derive(thiserror::Error, Debug)]
pub enum ParamTypeError {
    #[error("tuple type specified but no components found")]
    MissingComponents,
}

#[derive(PartialEq)]
pub enum GenerateAbiPropertiesType {
    PostgresWithDataTypes,
    PostgresColumnsNamesOnly,
    CsvHeaderNames,
    Object,
}

#[derive(Debug)]
pub struct GenerateAbiNamePropertiesResult {
    pub value: String,
    pub abi_type: String,
    #[allow(dead_code)]
    pub abi_name: String,
    pub ethereum_sql_type_wrapper: Option<EthereumSqlTypeWrapper>,
}

impl GenerateAbiNamePropertiesResult {
    pub fn new(value: String, name: &str, abi_type: &str) -> Self {
        Self {
            value,
            ethereum_sql_type_wrapper: solidity_type_to_ethereum_sql_type_wrapper(abi_type),
            abi_type: abi_type.to_string(),
            abi_name: name.to_string(),
        }
    }
}

impl ABIInput {
    pub fn format_param_type(&self) -> Result<String, ParamTypeError> {
        match self.type_.as_str() {
            "tuple" => {
                let components = self
                    .components
                    .as_ref()
                    .ok_or(ParamTypeError::MissingComponents)?;
                let formatted_components = components
                    .iter()
                    .map(|component| component.format_param_type())
                    .collect::<Result<Vec<_>, ParamTypeError>>()?
                    .join(",");
                Ok(format!("({})", formatted_components))
            }
            _ => Ok(self.type_.to_string()),
        }
    }

    pub fn generate_abi_name_properties(
        inputs: &[ABIInput],
        properties_type: &GenerateAbiPropertiesType,
        prefix: Option<&str>,
    ) -> Vec<GenerateAbiNamePropertiesResult> {
        inputs
            .iter()
            .flat_map(|input| {
                if let Some(components) = &input.components {
                    ABIInput::generate_abi_name_properties(
                        components,
                        properties_type,
                        Some(&camel_to_snake(&input.name)),
                    )
                } else {
                    match properties_type {
                        GenerateAbiPropertiesType::PostgresWithDataTypes => {
                            let value = format!(
                                "\"{}{}\" {}",
                                prefix.map_or_else(|| "".to_string(), |p| format!("{}_", p)),
                                camel_to_snake(&input.name),
                                solidity_type_to_db_type(&input.type_)
                            );

                            vec![GenerateAbiNamePropertiesResult::new(
                                value,
                                &input.name,
                                &input.type_,
                            )]
                        }
                        GenerateAbiPropertiesType::PostgresColumnsNamesOnly
                        | GenerateAbiPropertiesType::CsvHeaderNames => {
                            let value = format!(
                                "{}{}",
                                prefix.map_or_else(|| "".to_string(), |p| format!("{}_", p)),
                                camel_to_snake(&input.name),
                            );

                            vec![GenerateAbiNamePropertiesResult::new(
                                value,
                                &input.name,
                                &input.type_,
                            )]
                        }
                        GenerateAbiPropertiesType::Object => {
                            let value = format!(
                                "{}{}",
                                prefix.map_or_else(|| "".to_string(), |p| format!("{}.", p)),
                                camel_to_snake(&input.name),
                            );

                            vec![GenerateAbiNamePropertiesResult::new(
                                value,
                                &input.name,
                                &input.type_,
                            )]
                        }
                    }
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ABIItem {
    #[serde(default)]
    pub inputs: Vec<ABIInput>,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default)]
    pub type_: String,
}

#[derive(thiserror::Error, Debug)]
pub enum ReadAbiError {
    #[error("Could not read ABI string: {0}")]
    CouldNotReadAbiString(std::io::Error),

    #[error("Could not read ABI JSON: {0}")]
    CouldNotReadAbiJson(serde_json::Error),
}

impl ABIItem {
    pub fn format_event_signature(&self) -> Result<String, ParamTypeError> {
        let formatted_inputs = self
            .inputs
            .iter()
            .map(|component| component.format_param_type())
            .collect::<Result<Vec<_>, _>>()?;
        Ok(formatted_inputs.join(","))
    }

    pub fn extract_event_names_and_signatures_from_abi(
        abi_json: &[ABIItem],
    ) -> Result<Vec<EventInfo>, ParamTypeError> {
        let mut events = Vec::new();
        for item in abi_json.iter() {
            if item.type_ == "event" {
                let signature = item.format_event_signature()?;
                events.push(EventInfo::new(item, signature));
            }
        }
        Ok(events)
    }

    pub fn read_abi_items(
        project_path: &Path,
        contract: &Contract,
    ) -> Result<Vec<ABIItem>, ReadAbiError> {
        let full_path = get_full_path(project_path, &contract.abi);
        let abi_str = fs::read_to_string(full_path).map_err(ReadAbiError::CouldNotReadAbiString)?;
        let abi_items: Vec<ABIItem> =
            serde_json::from_str(&abi_str).map_err(ReadAbiError::CouldNotReadAbiJson)?;

        let filtered_abi_items = match &contract.include_events {
            Some(events) => abi_items
                .into_iter()
                .filter(|item| item.type_ != "event" || events.contains(&item.name))
                .collect(),
            None => abi_items,
        };

        Ok(filtered_abi_items)
    }

    pub fn get_abi_items(
        project_path: &Path,
        contract: &Contract,
        is_filter: bool,
    ) -> Result<Vec<ABIItem>, ReadAbiError> {
        let mut abi_items = ABIItem::read_abi_items(project_path, contract)?;
        if is_filter {
            let filter_event_names: Vec<String> = contract
                .details
                .iter()
                .filter_map(|detail| {
                    if let IndexingContractSetup::Filter(filter) = &detail.indexing_contract_setup()
                    {
                        Some(filter.event_name.clone())
                    } else {
                        None
                    }
                })
                .collect();

            abi_items = abi_items
                .iter()
                .filter(|item| item.type_ == "event" && filter_event_names.contains(&item.name))
                .cloned()
                .collect();
        }

        Ok(abi_items)
    }
}

#[derive(Debug, Clone)]
pub struct EventInfo {
    pub name: String,
    pub inputs: Vec<ABIInput>,
    signature: String,
    struct_result: String,
    struct_data: String,
}

#[derive(thiserror::Error, Debug)]
pub enum CreateCsvFileForEvent {
    #[error("Could not create the dir {0}")]
    CreateDirFailed(std::io::Error),
}

impl EventInfo {
    pub fn new(item: &ABIItem, signature: String) -> Self {
        EventInfo {
            name: item.name.clone(),
            inputs: item.inputs.clone(),
            signature,
            struct_result: format!("{}Result", item.name),
            struct_data: format!("{}Data", item.name),
        }
    }

    pub fn topic_id(&self) -> String {
        let event_signature = format!("{}({})", self.name, self.signature);
        Map::collect(
            keccak256(event_signature)
                .iter()
                .map(|byte| format!("{:02x}", byte)),
        )
    }

    pub fn struct_result(&self) -> &str {
        &self.struct_result
    }

    pub fn struct_data(&self) -> &str {
        &self.struct_data
    }

    pub fn csv_headers_for_event(&self) -> Vec<String> {
        let mut headers: Vec<String> = ABIInput::generate_abi_name_properties(
            &self.inputs,
            &GenerateAbiPropertiesType::CsvHeaderNames,
            None,
        )
        .iter()
        .map(|m| m.value.clone())
        .collect();

        headers.insert(0, r#"contract_address"#.to_string());
        headers.push(r#"tx_hash"#.to_string());
        headers.push(r#"block_number"#.to_string());
        headers.push(r#"block_hash"#.to_string());
        headers.push(r#"network"#.to_string());
        headers.push(r#"tx_index"#.to_string());
        headers.push(r#"log_index"#.to_string());

        headers
    }

    pub fn create_csv_file_for_event(
        &self,
        project_path: &Path,
        contract: &Contract,
        csv_path: &str,
    ) -> Result<String, CreateCsvFileForEvent> {
        let csv_file_name = format!("{}-{}.csv", contract.name, self.name).to_lowercase();
        let csv_folder = project_path.join(format!("{}/{}", csv_path, contract.name));

        // Create directory if it does not exist.
        if let Err(e) = fs::create_dir_all(&csv_folder) {
            return Err(CreateCsvFileForEvent::CreateDirFailed(e));
        }

        // Create last-synced-blocks if it does not exist.
        if let Err(e) = fs::create_dir_all(csv_folder.join("last-synced-blocks")) {
            return Err(CreateCsvFileForEvent::CreateDirFailed(e));
        }

        Ok(format!("{}/{}", csv_folder.display(), csv_file_name))
    }
}