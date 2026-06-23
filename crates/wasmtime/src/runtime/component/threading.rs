use crate::component::InstancePre;
use crate::component::instance::lookup_vmexport;
use crate::component::store::ComponentInstanceId;
use crate::runtime::vm::component::ComponentInstance;
use crate::runtime::vm::{
    self, SendSyncPtr, SharedMemory, VMContext, VMGlobalDefinition, VMGlobalImport, VMGlobalKind,
    VMMemoryDefinition, VMOpaqueContext, VMTableDefinition, VMTableImport,
};
use crate::store::{ComponentThreadStoreDataFactory, InstanceId, StoreOpaque};
use crate::{Result, Store, bail};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::any::{Any, TypeId};
use core::pin::Pin;
use core::ptr::NonNull;
use wasmtime_environ::component::{
    CanonicalOptionsDataModel, GlobalInitializer, RuntimeInstanceIndex, RuntimeMemoryIndex,
    RuntimeTableIndex,
};
use wasmtime_environ::{
    DefinedGlobalIndex, DefinedMemoryIndex, DefinedTableIndex, EntityRef, GlobalIndex, MemoryIndex,
    TableIndex,
};

/// Fork-local scaffold for preemptive Component Model `thread.spawn-*`.
///
/// This is not an execution object yet. It records the component instance
/// information that a future OS-thread path must preserve when creating
/// per-thread execution state.
pub(crate) struct ComponentThreadTemplate<T: 'static> {
    instance_pre: InstancePre<T>,
    state: ComponentThreadRuntimeState,
    instantiated_core_modules: u32,
}

/// Runtime objects that are visible through the component VM context.
///
/// These are only the runtime memory/table slots that the canonical ABI has
/// extracted into `VMComponentContext`. This is intentionally narrower than all
/// core module state in the component, and it is not yet filtered down to only
/// objects that are safe to share across host threads.
pub(crate) struct ComponentThreadRuntimeState {
    parent_core_instances: Vec<ComponentThreadCoreInstance>,
    core_shared_memories: Vec<ComponentThreadCoreMemory>,
    core_shared_tables: Vec<ComponentThreadCoreTable>,
    core_shared_globals: Vec<ComponentThreadCoreGlobal>,
    core_unshared_mutable_globals: Vec<ComponentThreadCoreMutableGlobal>,
    runtime_memories: Vec<ComponentThreadMemory>,
    runtime_tables: Vec<ComponentThreadTable>,
    component_resources: ComponentThreadResourceState,
    component_gc_options: u32,
}

pub(crate) struct ComponentThreadSpawnPlan<T: 'static> {
    instance_pre: InstancePre<T>,
    store_data_factory: ComponentThreadStoreDataFactory<T>,
    core_shared_memories: Vec<ComponentThreadCoreMemory>,
    core_shared_tables: Vec<ComponentThreadCoreTable>,
    core_shared_globals: Vec<ComponentThreadCoreGlobal>,
    runtime_tables: Vec<ComponentThreadTable>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ComponentThreadMemoryShareability {
    Shared,
    Unshared,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ComponentThreadTableShareability {
    Shared,
    Unshared,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ComponentThreadResourceState {
    total: u32,
    imported: u32,
    defined: u32,
}

/// Positive subset of component state the fork-local unsafe OS-thread backend
/// is allowed to share for the current Vibe experiment.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ComponentThreadSharedOwnershipSubset {
    shared_core_memories: usize,
    runtime_start_tables: usize,
    fixed_core_shared_tables: usize,
    growable_imported_runtime_start_tables: usize,
    shared_global_definitions: usize,
    direct_defined_mutable_shared_global_flushbacks: usize,
}

/// A core wasm instance owned by the parent component instance.
pub(crate) struct ComponentThreadCoreInstance {
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    runtime_index: RuntimeInstanceIndex,
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    instance: InstanceId,
}

/// A shared memory defined by a parent core wasm instance.
#[derive(Clone)]
pub(crate) struct ComponentThreadCoreMemory {
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    runtime_instance: RuntimeInstanceIndex,
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    defined_index: DefinedMemoryIndex,
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    definition: SendSyncPtr<VMMemoryDefinition>,
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    shared_memory: SharedMemory,
}

/// A shared table defined by a parent core wasm instance.
#[derive(Clone)]
pub(crate) struct ComponentThreadCoreTable {
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    runtime_instance: RuntimeInstanceIndex,
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    defined_index: DefinedTableIndex,
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    definition: SendSyncPtr<VMTableDefinition>,
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    vmctx: SendSyncPtr<VMContext>,
    #[allow(
        dead_code,
        reason = "checked by the unsafe OS-thread path before rebinding"
    )]
    growable: bool,
    owner_has_defined_funcs: bool,
}

/// A shared global defined by a parent core wasm instance.
#[derive(Clone)]
pub(crate) struct ComponentThreadCoreGlobal {
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    runtime_instance: RuntimeInstanceIndex,
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    defined_index: DefinedGlobalIndex,
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    definition: SendSyncPtr<VMGlobalDefinition>,
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    vmctx: SendSyncPtr<VMContext>,
    mutability: bool,
}

/// A mutable unshared global observed in a parent core wasm instance.
pub(crate) struct ComponentThreadCoreMutableGlobal {
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    runtime_instance: RuntimeInstanceIndex,
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    index: GlobalIndex,
}

/// A runtime memory pointer captured from a component instance.
pub(crate) struct ComponentThreadMemory {
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    index: RuntimeMemoryIndex,
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    source_runtime_instance: RuntimeInstanceIndex,
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    source_parent_instance: InstanceId,
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    definition: NonNull<VMMemoryDefinition>,
    #[allow(
        dead_code,
        reason = "checked by the future OS-thread path before rebinding"
    )]
    shareability: ComponentThreadMemoryShareability,
}

/// A runtime table import captured from a component instance.
#[derive(Clone)]
pub(crate) struct ComponentThreadTable {
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    index: RuntimeTableIndex,
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    source_runtime_instance: RuntimeInstanceIndex,
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    source_parent_instance: InstanceId,
    #[allow(
        dead_code,
        reason = "state is captured before the OS-thread path uses it"
    )]
    import: VMTableImport,
    #[allow(
        dead_code,
        reason = "checked by the future OS-thread path before rebinding"
    )]
    shareability: ComponentThreadTableShareability,
    #[allow(
        dead_code,
        reason = "checked by the unsafe OS-thread path before rebinding"
    )]
    growable: bool,
}

fn table_limits_growable(min: u64, max: Option<u64>) -> bool {
    max.is_none_or(|max| max > min)
}

impl<T: 'static> ComponentThreadTemplate<T> {
    pub(crate) fn new(
        instance_pre: InstancePre<T>,
        store: &mut StoreOpaque,
        component_instance: ComponentInstanceId,
    ) -> ComponentThreadTemplate<T> {
        let (
            parent_core_instances,
            core_shared_memories,
            core_shared_tables,
            core_shared_globals,
            core_unshared_mutable_globals,
            memory_exports,
            table_exports,
            instantiated_core_modules,
            num_runtime_memories,
            num_runtime_tables,
            component_resources,
            component_gc_options,
        ) = {
            let instance = store.component_instance(component_instance);
            let env_component = instance.component().env_component();

            let mut parent_core_instances =
                Vec::with_capacity(env_component.num_runtime_instances as usize);
            for i in 0..env_component.num_runtime_instances {
                let runtime_index = RuntimeInstanceIndex::from_u32(i);
                parent_core_instances.push(ComponentThreadCoreInstance {
                    runtime_index,
                    instance: instance.instance(runtime_index),
                });
            }

            let mut core_shared_memories = Vec::new();
            let mut core_shared_tables = Vec::new();
            let mut core_shared_globals = Vec::new();
            let mut core_unshared_mutable_globals = Vec::new();
            for core_instance in &parent_core_instances {
                let instance = store.instance(core_instance.instance);
                let module = instance.env_module();
                for i in 0..module.num_defined_memories() {
                    let defined_index = DefinedMemoryIndex::new(i);
                    let memory_index = module.memory_index(defined_index);
                    if !module.memories[memory_index].shared {
                        continue;
                    }
                    let shared_memory = instance
                        .get_defined_memory(defined_index)
                        .as_shared_memory()
                        .expect("shared memory type must use SharedMemory storage")
                        .clone();
                    core_shared_memories.push(ComponentThreadCoreMemory {
                        runtime_instance: core_instance.runtime_index,
                        defined_index,
                        definition: SendSyncPtr::new(instance.memory_ptr(defined_index)),
                        shared_memory,
                    });
                }

                for i in 0..module.num_defined_tables() {
                    let defined_index = DefinedTableIndex::new(i);
                    let table_index = module.table_index(defined_index);
                    let table = module.tables[table_index];
                    if !table.shared {
                        continue;
                    }
                    core_shared_tables.push(ComponentThreadCoreTable {
                        runtime_instance: core_instance.runtime_index,
                        defined_index,
                        definition: SendSyncPtr::new(instance.table_ptr(defined_index)),
                        vmctx: SendSyncPtr::new(instance.vmctx()),
                        growable: table_limits_growable(table.limits.min, table.limits.max),
                        owner_has_defined_funcs: module.defined_func_indices().next().is_some(),
                    });
                }

                for (index, global) in module.globals.iter() {
                    if global.shared {
                        if let Some(defined_index) = module.defined_global_index(index) {
                            core_shared_globals.push(ComponentThreadCoreGlobal {
                                runtime_instance: core_instance.runtime_index,
                                defined_index,
                                definition: SendSyncPtr::new(instance.global_ptr(defined_index)),
                                vmctx: SendSyncPtr::new(instance.vmctx()),
                                mutability: global.mutability,
                            });
                        }
                    } else if global.mutability {
                        core_unshared_mutable_globals.push(ComponentThreadCoreMutableGlobal {
                            runtime_instance: core_instance.runtime_index,
                            index,
                        });
                    }
                }
            }

            let memory_exports = env_component
                .initializers
                .iter()
                .filter_map(|initializer| {
                    let GlobalInitializer::ExtractMemory(memory) = initializer else {
                        return None;
                    };
                    Some((memory.index, memory.export.clone()))
                })
                .collect::<Vec<_>>();

            let table_exports = env_component
                .initializers
                .iter()
                .filter_map(|initializer| {
                    let GlobalInitializer::ExtractTable(table) = initializer else {
                        return None;
                    };
                    Some((table.index, table.export.clone()))
                })
                .collect::<Vec<_>>();

            let instantiated_core_modules = env_component
                .initializers
                .iter()
                .filter(|initializer| {
                    matches!(initializer, GlobalInitializer::InstantiateModule(..))
                })
                .count()
                .try_into()
                .unwrap_or(u32::MAX);

            let component_resources = ComponentThreadResourceState {
                total: env_component.num_resources,
                imported: env_component
                    .imported_resources
                    .len()
                    .try_into()
                    .unwrap_or(u32::MAX),
                defined: env_component
                    .defined_resource_instances
                    .len()
                    .try_into()
                    .unwrap_or(u32::MAX),
            };
            let component_gc_options = env_component
                .options
                .values()
                .filter(|options| matches!(options.data_model, CanonicalOptionsDataModel::Gc {}))
                .count()
                .try_into()
                .unwrap_or(u32::MAX);

            (
                parent_core_instances,
                core_shared_memories,
                core_shared_tables,
                core_shared_globals,
                core_unshared_mutable_globals,
                memory_exports,
                table_exports,
                instantiated_core_modules,
                env_component.num_runtime_memories,
                env_component.num_runtime_tables,
                component_resources,
                component_gc_options,
            )
        };

        let mut runtime_memories = Vec::with_capacity(num_runtime_memories as usize);
        for (index, export) in memory_exports {
            let source_runtime_instance = export.instance;
            let shareability = match lookup_vmexport(store, component_instance, &export) {
                vm::Export::SharedMemory(..) => ComponentThreadMemoryShareability::Shared,
                vm::Export::Memory(_) => ComponentThreadMemoryShareability::Unshared,
                _ => unreachable!("ExtractMemory must resolve to a core memory export"),
            };

            let instance = store.component_instance(component_instance);
            runtime_memories.push(ComponentThreadMemory {
                index,
                source_runtime_instance,
                source_parent_instance: instance.instance(source_runtime_instance),
                definition: instance.runtime_memory(index),
                shareability,
            });
        }

        let mut runtime_tables = Vec::with_capacity(num_runtime_tables as usize);
        for (index, export) in table_exports {
            let source_runtime_instance = export.instance;
            let (shareability, growable) = match lookup_vmexport(store, component_instance, &export)
            {
                vm::Export::Table(table) => {
                    let ty = table.wasmtime_ty(store);
                    let shareability = if ty.shared {
                        ComponentThreadTableShareability::Shared
                    } else {
                        ComponentThreadTableShareability::Unshared
                    };
                    let growable = table_limits_growable(ty.limits.min, ty.limits.max);
                    (shareability, growable)
                }
                _ => unreachable!("ExtractTable must resolve to a core table export"),
            };

            let instance = store.component_instance(component_instance);
            runtime_tables.push(ComponentThreadTable {
                index,
                source_runtime_instance,
                source_parent_instance: instance.instance(source_runtime_instance),
                import: instance.runtime_table(index),
                shareability,
                growable,
            });
        }

        ComponentThreadTemplate {
            instance_pre,
            state: ComponentThreadRuntimeState {
                parent_core_instances,
                core_shared_memories,
                core_shared_tables,
                core_shared_globals,
                core_unshared_mutable_globals,
                runtime_memories,
                runtime_tables,
                component_resources,
                component_gc_options,
            },
            instantiated_core_modules,
        }
    }

    pub(crate) fn instance_pre(&self) -> &InstancePre<T> {
        &self.instance_pre
    }

    pub(crate) fn runtime_state(&self) -> &ComponentThreadRuntimeState {
        &self.state
    }

    pub(crate) fn spawn_plan_with_store_data_factory(
        &self,
        store_data_factory: Option<ComponentThreadStoreDataFactory<T>>,
    ) -> Result<Option<ComponentThreadSpawnPlan<T>>> {
        if self.state.core_shared_memories.is_empty()
            && self.state.core_shared_tables.is_empty()
            && self.state.core_shared_globals.is_empty()
        {
            return Ok(None);
        }

        let runtime_tables = self
            .state
            .runtime_tables
            .iter()
            .filter(|table| table.shareability == ComponentThreadTableShareability::Shared)
            .cloned()
            .collect();

        let Some(store_data_factory) =
            store_data_factory.or_else(default_unit_store_data_factory::<T>)
        else {
            bail!(
                "fork-local Component Model OS-thread spawn requires a per-thread store-data \
                 factory for non-Store<()> embedder data"
            );
        };

        Ok(Some(ComponentThreadSpawnPlan {
            instance_pre: self.instance_pre.clone(),
            store_data_factory,
            core_shared_memories: self.state.core_shared_memories.clone(),
            core_shared_tables: self.state.core_shared_tables.clone(),
            core_shared_globals: self.state.core_shared_globals.clone(),
            runtime_tables,
        }))
    }

    pub(crate) fn instantiated_core_modules(&self) -> u32 {
        self.instantiated_core_modules
    }

    pub(crate) fn requires_core_instance_state_sharing(&self) -> bool {
        self.instantiated_core_modules != 0
    }

    #[allow(
        dead_code,
        reason = "fork-local guard for the OS-thread path once it is wired"
    )]
    pub(crate) fn validate_rebindable_runtime_state(&self) -> Result<()> {
        for memory in self.state.runtime_memories() {
            if memory.shareability != ComponentThreadMemoryShareability::Shared {
                bail!(
                    "component thread runtime rebind rejected: unshared runtime memory {:?} \
                     extracted from core instance {:?}",
                    memory.index,
                    memory.source_runtime_instance,
                );
            }
        }

        for table in self.state.runtime_tables() {
            if table.shareability != ComponentThreadTableShareability::Shared {
                bail!(
                    "component thread runtime rebind rejected: unshared runtime table {:?} \
                     extracted from core instance {:?}",
                    table.index,
                    table.source_runtime_instance,
                );
            }
        }

        for global in self.state.core_unshared_mutable_globals() {
            bail!(
                "component thread runtime rebind rejected: mutable unshared global {:?} \
                 in core instance {:?}",
                global.index,
                global.runtime_instance,
            );
        }

        Ok(())
    }

    #[allow(
        dead_code,
        reason = "fork-local guard for the opt-in unsafe OS-thread experiment"
    )]
    pub(crate) fn validate_unsafe_preemptive_spawn_indirect(
        &self,
        start_func_table_idx: RuntimeTableIndex,
        start_func_vmctx: Option<NonNull<VMOpaqueContext>>,
    ) -> Result<()> {
        self.unsafe_preemptive_shared_ownership_subset(
            Some(start_func_table_idx),
            start_func_vmctx,
        )?;
        Ok(())
    }

    pub(crate) fn validate_unsafe_preemptive_spawn_ref(
        &self,
        start_func_vmctx: Option<NonNull<VMOpaqueContext>>,
    ) -> Result<()> {
        self.unsafe_preemptive_shared_ownership_subset(None, start_func_vmctx)?;
        Ok(())
    }

    pub(crate) fn unsafe_preemptive_shared_ownership_subset(
        &self,
        start_func_table_idx: Option<RuntimeTableIndex>,
        start_func_vmctx: Option<NonNull<VMOpaqueContext>>,
    ) -> Result<ComponentThreadSharedOwnershipSubset> {
        let resources = self.state.component_resources();
        if resources.total != 0 {
            bail!(
                "component thread preemptive spawn rejected: component resources are not yet \
                 supported (total {}, imported {}, defined {})",
                resources.total,
                resources.imported,
                resources.defined,
            );
        }

        let component_gc_options = self.state.component_gc_options();
        if component_gc_options != 0 {
            bail!(
                "component thread preemptive spawn rejected: component-model GC canonical \
                 options are not yet supported ({component_gc_options})",
            );
        }

        let mut subset = ComponentThreadSharedOwnershipSubset {
            shared_core_memories: self.state.core_shared_memories().len(),
            shared_global_definitions: self.state.core_shared_globals().len(),
            ..Default::default()
        };

        for memory in self.state.runtime_memories() {
            if memory.shareability != ComponentThreadMemoryShareability::Shared {
                bail!(
                    "component thread preemptive spawn rejected: unshared runtime memory {:?} \
                 extracted from core instance {:?}",
                    memory.index,
                    memory.source_runtime_instance,
                );
            }
        }

        let mut runtime_start_table_definitions = Vec::new();
        for table in self.state.runtime_tables() {
            if table.shareability != ComponentThreadTableShareability::Shared {
                bail!(
                    "component thread preemptive spawn rejected: unshared runtime table {:?} \
                     extracted from core instance {:?}",
                    table.index,
                    table.source_runtime_instance,
                );
            }
            match start_func_table_idx {
                Some(start_func_table_idx) if table.index == start_func_table_idx => {}
                Some(start_func_table_idx) => {
                    bail!(
                        "component thread preemptive spawn rejected: runtime table {:?} is not \
                         the thread.spawn-indirect start table {:?}",
                        table.index,
                        start_func_table_idx,
                    );
                }
                None => {
                    bail!(
                        "component thread preemptive spawn-ref rejected: runtime table {:?} \
                         extracted from core instance {:?} is outside the current fork-local \
                         shared ownership subset",
                        table.index,
                        table.source_runtime_instance,
                    );
                }
            }
            subset.runtime_start_tables += 1;
            runtime_start_table_definitions.push(table.import.from.as_ptr());
        }

        for table in self.state.core_shared_tables() {
            if !table.growable {
                subset.fixed_core_shared_tables += 1;
                continue;
            }

            let Some(start_func_vmctx) = start_func_vmctx else {
                bail!(
                    "component thread preemptive spawn rejected: growable shared core table {:?} \
                     in core instance {:?} requires a known start function owner",
                    table.defined_index,
                    table.runtime_instance,
                );
            };

            if !runtime_start_table_definitions
                .iter()
                .any(|definition| *definition == table.definition.as_ptr())
            {
                bail!(
                    "component thread preemptive spawn rejected: growable shared core table {:?} \
                     in core instance {:?} is outside the Vibe shared ownership subset; only \
                     the imported runtime start table may be growable",
                    table.defined_index,
                    table.runtime_instance,
                );
            }

            if table.owner_vmctx() == start_func_vmctx {
                bail!(
                    "component thread preemptive spawn rejected: start function is defined \
                     in core instance {:?} that owns direct defined growable shared table {:?}; \
                     direct defined shared-table growth is not yet supported",
                    table.runtime_instance,
                    table.defined_index,
                );
            }

            if table.owner_has_defined_funcs {
                bail!(
                    "component thread preemptive spawn rejected: growable shared table owner \
                     core instance {:?} defines functions and may observe direct defined table \
                     {:?}; direct defined shared-table ownership is not yet supported",
                    table.runtime_instance,
                    table.defined_index,
                );
            }

            subset.growable_imported_runtime_start_tables += 1;
        }

        for global in self.state.core_unshared_mutable_globals() {
            bail!(
                "component thread preemptive spawn rejected: mutable unshared global {:?} \
                 in core instance {:?}",
                global.index,
                global.runtime_instance,
            );
        }

        if let Some(start_func_vmctx) = start_func_vmctx {
            subset.direct_defined_mutable_shared_global_flushbacks = self
                .state
                .core_shared_globals()
                .iter()
                .filter(|global| global.mutability && global.owner_vmctx() == start_func_vmctx)
                .count();
        }

        Ok(subset)
    }

    /// Rebinds the runtime memory/table slots of `instance` to the slots
    /// captured by this template.
    ///
    /// This is a low-level scaffold for the fork-local preemptive thread
    /// experiment. It does not make the target component instance safe to run
    /// on another host thread by itself.
    ///
    /// # Safety
    ///
    /// The caller must prove that the captured parent runtime slots remain
    /// valid for `instance`, and that using them from `instance` does not
    /// violate store, ownership, or thread-safety invariants.
    #[allow(
        dead_code,
        reason = "fork-local scaffold used once the OS-thread spawn path is wired"
    )]
    pub(crate) unsafe fn rebind_runtime_state_to(&self, mut instance: Pin<&mut ComponentInstance>) {
        for memory in self.state.runtime_memories() {
            unsafe {
                instance
                    .as_mut()
                    .component_thread_rebind_runtime_memory(memory.index, memory.definition);
            }
        }

        for table in self.state.runtime_tables() {
            unsafe {
                instance
                    .as_mut()
                    .component_thread_rebind_runtime_table(table.index, table.import);
            }
        }
    }
}

fn default_unit_store_data_factory<T: 'static>() -> Option<ComponentThreadStoreDataFactory<T>> {
    if TypeId::of::<T>() != TypeId::of::<()>() {
        return None;
    }

    Some(alloc::sync::Arc::new(|| {
        let value: Box<dyn Any> = Box::new(());
        match value.downcast::<T>() {
            Ok(value) => *value,
            Err(_) => unreachable!("TypeId check above ensures T is ()"),
        }
    }))
}

impl ComponentThreadRuntimeState {
    pub(crate) fn parent_core_instances(&self) -> &[ComponentThreadCoreInstance] {
        &self.parent_core_instances
    }

    pub(crate) fn core_shared_memories(&self) -> &[ComponentThreadCoreMemory] {
        &self.core_shared_memories
    }

    pub(crate) fn core_shared_tables(&self) -> &[ComponentThreadCoreTable] {
        &self.core_shared_tables
    }

    pub(crate) fn core_shared_globals(&self) -> &[ComponentThreadCoreGlobal] {
        &self.core_shared_globals
    }

    pub(crate) fn core_unshared_mutable_globals(&self) -> &[ComponentThreadCoreMutableGlobal] {
        &self.core_unshared_mutable_globals
    }

    pub(crate) fn runtime_memories(&self) -> &[ComponentThreadMemory] {
        &self.runtime_memories
    }

    pub(crate) fn runtime_tables(&self) -> &[ComponentThreadTable] {
        &self.runtime_tables
    }

    pub(crate) fn component_resources(&self) -> ComponentThreadResourceState {
        self.component_resources
    }

    pub(crate) fn component_gc_options(&self) -> u32 {
        self.component_gc_options
    }
}

impl<T: 'static> ComponentThreadSpawnPlan<T> {
    pub(crate) fn instance_pre(&self) -> &InstancePre<T> {
        &self.instance_pre
    }

    pub(crate) fn new_store(&self) -> Store<T> {
        Store::new(self.instance_pre.engine(), (self.store_data_factory)())
    }

    pub(crate) fn shared_memories_for_atomic_wait_interruption(&self) -> Vec<SharedMemory> {
        self.core_shared_memories
            .iter()
            .map(|memory| memory.shared_memory.clone())
            .collect()
    }

    /// Rebinds child core instances so their defined shared memories point at
    /// the parent shared memory allocations captured by this plan.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the child component instance was
    /// instantiated from this plan's `InstancePre`, and that running the child
    /// with the parent shared memories does not violate any store or component
    /// ownership invariants.
    pub(crate) unsafe fn rebind_core_shared_memories_to(
        &self,
        store: &mut StoreOpaque,
        component_instance: ComponentInstanceId,
    ) {
        for memory in &self.core_shared_memories {
            let parent_definition = memory.shared_memory.vmmemory_ptr();
            let core_instance = store
                .component_instance(component_instance)
                .instance(memory.runtime_instance);
            let source_vmctx = store.instance(core_instance).vmctx();
            let child = store.instance_mut(core_instance);
            unsafe {
                child.component_thread_rebind_defined_shared_memory(
                    memory.defined_index,
                    memory.shared_memory.clone(),
                );
            }

            let imported_memory_rebinds = {
                let component = store.component_instance(component_instance);
                let env_component = component.component().env_component();
                let mut imported_memory_rebinds = Vec::new();
                for i in 0..env_component.num_runtime_instances {
                    let runtime_instance = RuntimeInstanceIndex::from_u32(i);
                    let core_instance = component.instance(runtime_instance);
                    let instance = store.instance(core_instance);
                    let module = instance.env_module();
                    let mut imports = Vec::new();
                    for i in 0..module.num_imported_memories {
                        let imported_memory = MemoryIndex::new(i);
                        let import = instance.component_thread_imported_memory(imported_memory);
                        if import.vmctx.as_non_null() == source_vmctx
                            && import.index == memory.defined_index
                        {
                            imports.push(imported_memory);
                        }
                    }
                    if !imports.is_empty() {
                        imported_memory_rebinds.push((core_instance, imports));
                    }
                }
                imported_memory_rebinds
            };

            for (core_instance, imports) in imported_memory_rebinds {
                let mut child = store.instance_mut(core_instance);
                for imported_memory in imports {
                    unsafe {
                        child.as_mut().component_thread_rebind_imported_memory_from(
                            imported_memory,
                            parent_definition,
                        );
                    }
                }
            }
        }
    }

    /// Rebinds child core instances so their shared table VMContext slots point
    /// at the parent table allocation captured by this plan.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the child component instance was
    /// instantiated from this plan's `InstancePre`, and that running the child
    /// with the parent shared tables does not violate any store or component
    /// ownership invariants.
    pub(crate) unsafe fn rebind_core_shared_tables_to(
        &self,
        store: &mut StoreOpaque,
        component_instance: ComponentInstanceId,
    ) {
        for table in &self.core_shared_tables {
            let parent_definition = table.definition_value();
            let parent_import = table.import();
            let core_instance = store
                .component_instance(component_instance)
                .instance(table.runtime_instance);
            let source_vmctx = store.instance(core_instance).vmctx();
            let child = store.instance_mut(core_instance);
            unsafe {
                child.component_thread_rebind_defined_table(table.defined_index, parent_definition);
            }

            let imported_table_rebinds = {
                let component = store.component_instance(component_instance);
                let env_component = component.component().env_component();
                let mut imported_table_rebinds = Vec::new();
                for i in 0..env_component.num_runtime_instances {
                    let runtime_instance = RuntimeInstanceIndex::from_u32(i);
                    let core_instance = component.instance(runtime_instance);
                    let instance = store.instance(core_instance);
                    let module = instance.env_module();
                    let mut imports = Vec::new();
                    for i in 0..module.num_imported_tables {
                        let imported_table = TableIndex::new(i);
                        let import = instance.component_thread_imported_table(imported_table);
                        if import.vmctx.as_non_null() == source_vmctx
                            && import.index == table.defined_index
                        {
                            imports.push(imported_table);
                        }
                    }
                    if !imports.is_empty() {
                        imported_table_rebinds.push((core_instance, imports));
                    }
                }
                imported_table_rebinds
            };

            for (core_instance, imports) in imported_table_rebinds {
                let mut child = store.instance_mut(core_instance);
                for imported_table in imports {
                    unsafe {
                        child
                            .as_mut()
                            .component_thread_rebind_imported_table(imported_table, parent_import);
                    }
                }
            }
        }
    }

    /// Rebinds child imported globals so imports targeting child shared global
    /// definitions point at the parent shared global captured by this plan.
    ///
    /// Defined globals are stored inline in each core instance's VMContext, so
    /// this also copies the parent value into the child counterpart before
    /// import rebinding. That copy is only an initial value handoff; ongoing
    /// sharing for direct defined-global accesses requires a different VMContext
    /// representation.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the child component instance was
    /// instantiated from this plan's `InstancePre`, that copied values are
    /// type-compatible, and that using parent global definitions from child
    /// imports does not violate GC, store, or thread-safety invariants.
    pub(crate) unsafe fn rebind_core_shared_globals_to(
        &self,
        store: &mut StoreOpaque,
        component_instance: ComponentInstanceId,
    ) {
        for global in &self.core_shared_globals {
            let parent_definition = global.definition_value();
            let parent_import = global.import();
            let core_instance = store
                .component_instance(component_instance)
                .instance(global.runtime_instance);
            let source_definition = store
                .instance(core_instance)
                .global_ptr(global.defined_index);
            let child = store.instance_mut(core_instance);
            unsafe {
                child
                    .component_thread_write_defined_global(global.defined_index, parent_definition);
            }

            let imported_global_rebinds = {
                let component = store.component_instance(component_instance);
                let env_component = component.component().env_component();
                let mut imported_global_rebinds = Vec::new();
                for i in 0..env_component.num_runtime_instances {
                    let runtime_instance = RuntimeInstanceIndex::from_u32(i);
                    let core_instance = component.instance(runtime_instance);
                    let instance = store.instance(core_instance);
                    let module = instance.env_module();
                    let mut imports = Vec::new();
                    for i in 0..module.num_imported_globals {
                        let imported_global = GlobalIndex::new(i);
                        let import = instance.component_thread_imported_global(imported_global);
                        if import.from.as_non_null() == source_definition {
                            imports.push(imported_global);
                        }
                    }
                    if !imports.is_empty() {
                        imported_global_rebinds.push((core_instance, imports));
                    }
                }
                imported_global_rebinds
            };

            for (core_instance, imports) in imported_global_rebinds {
                let mut child = store.instance_mut(core_instance);
                for imported_global in imports {
                    unsafe {
                        child.as_mut().component_thread_rebind_imported_global(
                            imported_global,
                            parent_import,
                        );
                    }
                }
            }
        }
    }

    /// Copies direct defined mutable shared-global writes from the child start
    /// function owner back to the parent definitions captured by this plan.
    ///
    /// This is a fork-local diagnostic bridge for defined globals, whose
    /// storage is inline in each core instance's VMContext. Imported shared
    /// globals already point at the parent definition after
    /// `rebind_core_shared_globals_to`; this only handles direct defined-global
    /// accesses performed by the start function's own core instance.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the child component instance was
    /// instantiated from this plan's `InstancePre`, that the start function has
    /// finished its direct global accesses, and that copying raw global
    /// definitions across stores does not violate GC or reference ownership
    /// invariants.
    pub(crate) unsafe fn flush_direct_defined_shared_globals_from(
        &self,
        store: &mut StoreOpaque,
        component_instance: ComponentInstanceId,
        runtime_instance: RuntimeInstanceIndex,
    ) {
        for global in &self.core_shared_globals {
            if !global.mutability || global.runtime_instance != runtime_instance {
                continue;
            }

            let core_instance = store
                .component_instance(component_instance)
                .instance(global.runtime_instance);
            let child_value = store
                .instance(core_instance)
                .component_thread_read_defined_global(global.defined_index);
            unsafe {
                global.definition.as_non_null().as_ptr().write(child_value);
            }
        }
    }

    /// Rebinds shared runtime table slots in the child component instance to
    /// the parent runtime table imports captured by this plan.
    ///
    /// # Safety
    ///
    /// The caller must ensure that every captured runtime table is shared and
    /// type-compatible with the child instance's corresponding runtime table.
    pub(crate) unsafe fn rebind_shared_runtime_tables_to(
        &self,
        store: &mut StoreOpaque,
        component_instance: ComponentInstanceId,
    ) {
        let mut instance = store.component_instance_mut(component_instance);
        for table in &self.runtime_tables {
            unsafe {
                instance
                    .as_mut()
                    .component_thread_rebind_runtime_table(table.index, table.import);
            }
        }
    }
}

impl ComponentThreadCoreMemory {
    #[allow(
        dead_code,
        reason = "field accessors are for the next OS-thread rebind layer"
    )]
    pub(crate) fn runtime_instance(&self) -> RuntimeInstanceIndex {
        self.runtime_instance
    }

    #[allow(
        dead_code,
        reason = "field accessors are for the next OS-thread rebind layer"
    )]
    pub(crate) fn defined_index(&self) -> DefinedMemoryIndex {
        self.defined_index
    }

    #[allow(
        dead_code,
        reason = "field accessors are for the next OS-thread rebind layer"
    )]
    pub(crate) fn definition(&self) -> NonNull<VMMemoryDefinition> {
        self.definition.as_non_null()
    }
}

impl ComponentThreadCoreTable {
    #[allow(
        dead_code,
        reason = "field accessors are for the next OS-thread rebind layer"
    )]
    pub(crate) fn runtime_instance(&self) -> RuntimeInstanceIndex {
        self.runtime_instance
    }

    #[allow(
        dead_code,
        reason = "field accessors are for the next OS-thread rebind layer"
    )]
    pub(crate) fn defined_index(&self) -> DefinedTableIndex {
        self.defined_index
    }

    #[allow(
        dead_code,
        reason = "field accessors are for the next OS-thread rebind layer"
    )]
    pub(crate) fn definition(&self) -> NonNull<VMTableDefinition> {
        self.definition.as_non_null()
    }

    pub(crate) fn definition_value(&self) -> VMTableDefinition {
        unsafe { self.definition.as_non_null().as_ptr().read() }
    }

    pub(crate) fn import(&self) -> VMTableImport {
        VMTableImport {
            from: self.definition.into(),
            vmctx: self.vmctx.into(),
            index: self.defined_index,
        }
    }

    fn owner_vmctx(&self) -> NonNull<VMOpaqueContext> {
        VMOpaqueContext::from_vmcontext(self.vmctx.as_non_null())
    }
}

impl ComponentThreadCoreGlobal {
    #[allow(
        dead_code,
        reason = "field accessors are for the next OS-thread rebind layer"
    )]
    pub(crate) fn runtime_instance(&self) -> RuntimeInstanceIndex {
        self.runtime_instance
    }

    #[allow(
        dead_code,
        reason = "field accessors are for the next OS-thread rebind layer"
    )]
    pub(crate) fn defined_index(&self) -> DefinedGlobalIndex {
        self.defined_index
    }

    #[allow(
        dead_code,
        reason = "field accessors are for the next OS-thread rebind layer"
    )]
    pub(crate) fn definition(&self) -> NonNull<VMGlobalDefinition> {
        self.definition.as_non_null()
    }

    pub(crate) fn definition_value(&self) -> VMGlobalDefinition {
        unsafe { self.definition.as_non_null().as_ptr().read() }
    }

    pub(crate) fn import(&self) -> VMGlobalImport {
        VMGlobalImport {
            from: self.definition.into(),
            vmctx: Some(VMOpaqueContext::from_vmcontext(self.vmctx.as_non_null()).into()),
            kind: VMGlobalKind::Instance(self.defined_index),
        }
    }

    fn owner_vmctx(&self) -> NonNull<VMOpaqueContext> {
        VMOpaqueContext::from_vmcontext(self.vmctx.as_non_null())
    }
}

impl ComponentThreadMemory {
    #[allow(
        dead_code,
        reason = "field accessors are for the next OS-thread rebind layer"
    )]
    pub(crate) fn source_runtime_instance(&self) -> RuntimeInstanceIndex {
        self.source_runtime_instance
    }

    #[allow(
        dead_code,
        reason = "field accessors are for the next OS-thread rebind layer"
    )]
    pub(crate) fn source_parent_instance(&self) -> InstanceId {
        self.source_parent_instance
    }
}

impl ComponentThreadTable {
    #[allow(
        dead_code,
        reason = "field accessors are for the next OS-thread rebind layer"
    )]
    pub(crate) fn source_runtime_instance(&self) -> RuntimeInstanceIndex {
        self.source_runtime_instance
    }

    #[allow(
        dead_code,
        reason = "field accessors are for the next OS-thread rebind layer"
    )]
    pub(crate) fn source_parent_instance(&self) -> InstanceId {
        self.source_parent_instance
    }

    #[allow(
        dead_code,
        reason = "field accessors are for the next OS-thread rebind layer"
    )]
    pub(crate) fn shareability(&self) -> ComponentThreadTableShareability {
        self.shareability
    }
}

#[cfg(test)]
impl ComponentThreadSharedOwnershipSubset {
    pub(crate) fn shared_core_memories(&self) -> usize {
        self.shared_core_memories
    }

    pub(crate) fn runtime_start_tables(&self) -> usize {
        self.runtime_start_tables
    }

    pub(crate) fn fixed_core_shared_tables(&self) -> usize {
        self.fixed_core_shared_tables
    }

    pub(crate) fn growable_imported_runtime_start_tables(&self) -> usize {
        self.growable_imported_runtime_start_tables
    }

    pub(crate) fn shared_global_definitions(&self) -> usize {
        self.shared_global_definitions
    }

    pub(crate) fn direct_defined_mutable_shared_global_flushbacks(&self) -> usize {
        self.direct_defined_mutable_shared_global_flushbacks
    }
}

#[cfg(all(test, feature = "component-model-async"))]
mod tests {
    use super::ComponentThreadTableShareability;
    use crate::component::{Component, Linker};
    use crate::runtime::vm::{VMGlobalDefinition, VMOpaqueContext};
    use crate::{AsContextMut, Config, Engine, Result, Store};
    use wasmtime_environ::component::{RuntimeInstanceIndex, RuntimeTableIndex};
    use wasmtime_environ::{DefinedGlobalIndex, EntityRef, GlobalIndex, MemoryIndex, TableIndex};

    fn thread_template_component(engine: &Engine) -> Result<Component> {
        Component::new(
            engine,
            r#"
            (component
                (core module $libc
                    (memory (export "memory") 1)
                    (table $table (export "__indirect_function_table") shared 1 1
                        (ref null (shared func))))

                (core module $m
                    (type $start-func-ty (shared (func (param i32))))
                    (type $spawn-indirect-ty
                        (shared (func (param i32 i32) (result i32))))
                    (import "" "thread.spawn-indirect"
                        (func $thread-spawn-indirect (type $spawn-indirect-ty)))
                    (import "libc" "memory" (memory $memory 1))
                    (import "libc" "__indirect_function_table"
                        (table $table shared 1 1 (ref null (shared func))))
                    (func $thread-start (type $start-func-ty))
                    (elem (table $table) (i32.const 0)
                        (ref null (shared func)) (ref.func $thread-start))
                    (func (export "realloc")
                        (param i32 i32 i32 i32)
                        (result i32)
                        i32.const 0)
                    (func (export "empty-string") (result i32)
                        i32.const 0)
                    (export "memory" (memory $memory)))

                (core instance $libc (instantiate $libc))
                (core type $start-func-ty (shared (func (param i32))))
                (alias core export $libc "memory" (core memory $memory))
                (alias core export $libc "__indirect_function_table"
                    (core table $table))
                (core func $thread-spawn-indirect
                    (canon thread.spawn-indirect $start-func-ty (table $table)))
                (core instance $i
                    (instantiate $m
                        (with "" (instance
                            (export "thread.spawn-indirect"
                                (func $thread-spawn-indirect))))
                        (with "libc" (instance
                            (export "memory" (memory $memory))
                            (export "__indirect_function_table" (table $table))))))

                (func (export "empty-string") (result string)
                    (canon lift (core func $i "empty-string")
                        (memory $i "memory")
                        (realloc (func $i "realloc"))))
            )
            "#,
        )
    }

    fn thread_spawn_table_component(engine: &Engine) -> Result<Component> {
        Component::new(
            engine,
            r#"
            (component
                (core module $libc
                    (memory (export "mem") 1 1 shared)
                    (table $table (export "__indirect_function_table") shared 1 1
                        (ref null (shared func))))

                (core module $m
                    (type $start-func-ty (shared (func (param i32))))
                    (type $spawn-indirect-ty
                        (shared (func (param i32 i32) (result i32))))
                    (type $run-ty (func (result i32)))
                    (import "" "thread.spawn-indirect"
                        (func $thread-spawn-indirect (type $spawn-indirect-ty)))
                    (import "libc" "mem" (memory $mem 1 1 shared))
                    (import "libc" "__indirect_function_table"
                        (table $table shared 1 1 (ref null (shared func))))
                    (func $thread-start (type $start-func-ty))
                    (elem (table $table) (i32.const 0)
                        (ref null (shared func)) (ref.func $thread-start))
                    (func (export "run") (type $run-ty)
                        i32.const 0))

                (core instance $libc (instantiate $libc))
                (core type $start-func-ty (shared (func (param i32))))
                (alias core export $libc "mem" (core memory $mem))
                (alias core export $libc "__indirect_function_table"
                    (core table $table))
                (core func $thread-spawn-indirect
                    (canon thread.spawn-indirect $start-func-ty (table $table)))
                (core instance $i
                    (instantiate $m
                        (with "" (instance
                            (export "thread.spawn-indirect"
                                (func $thread-spawn-indirect))))
                        (with "libc" (instance
                            (export "mem" (memory $mem))
                            (export "__indirect_function_table" (table $table))))))

                (func (export "run") async (result u32)
                    (canon lift (core func $i "run")))
            )
            "#,
        )
    }

    fn thread_spawn_resource_component(engine: &Engine) -> Result<Component> {
        Component::new(
            engine,
            r#"
            (component
                (type $r (resource (rep i32)))

                (core module $libc
                    (memory (export "mem") 1 1 shared)
                    (table $table (export "__indirect_function_table") shared 1 1
                        (ref null (shared func))))

                (core module $m
                    (type $start-func-ty (shared (func (param i32))))
                    (type $spawn-indirect-ty
                        (shared (func (param i32 i32) (result i32))))
                    (type $run-ty (func (result i32)))
                    (import "" "thread.spawn-indirect"
                        (func $thread-spawn-indirect (type $spawn-indirect-ty)))
                    (import "libc" "mem" (memory $mem 1 1 shared))
                    (import "libc" "__indirect_function_table"
                        (table $table shared 1 1 (ref null (shared func))))
                    (func $thread-start (type $start-func-ty))
                    (elem (table $table) (i32.const 0)
                        (ref null (shared func)) (ref.func $thread-start))
                    (func (export "run") (type $run-ty)
                        i32.const 0))

                (core instance $libc (instantiate $libc))
                (core type $start-func-ty (shared (func (param i32))))
                (alias core export $libc "mem" (core memory $mem))
                (alias core export $libc "__indirect_function_table"
                    (core table $table))
                (core func $thread-spawn-indirect
                    (canon thread.spawn-indirect $start-func-ty (table $table)))
                (core instance $i
                    (instantiate $m
                        (with "" (instance
                            (export "thread.spawn-indirect"
                                (func $thread-spawn-indirect))))
                        (with "libc" (instance
                            (export "mem" (memory $mem))
                            (export "__indirect_function_table" (table $table))))))

                (func (export "run") async (result u32)
                    (canon lift (core func $i "run")))
                (export "r" (type $r))
            )
            "#,
        )
    }

    fn thread_spawn_gc_component(engine: &Engine) -> Result<Component> {
        Component::new(
            engine,
            r#"
            (component
                (core module $libc
                    (memory (export "mem") 1 1 shared)
                    (table $table (export "__indirect_function_table") shared 1 1
                        (ref null (shared func))))

                (core module $m
                    (type $start-func-ty (shared (func (param i32))))
                    (type $spawn-indirect-ty
                        (shared (func (param i32 i32) (result i32))))
                    (type $run-ty (func (result i32)))
                    (import "" "thread.spawn-indirect"
                        (func $thread-spawn-indirect (type $spawn-indirect-ty)))
                    (import "libc" "mem" (memory $mem 1 1 shared))
                    (import "libc" "__indirect_function_table"
                        (table $table shared 1 1 (ref null (shared func))))
                    (func $thread-start (type $start-func-ty))
                    (elem (table $table) (i32.const 0)
                        (ref null (shared func)) (ref.func $thread-start))
                    (func (export "run") (type $run-ty)
                        i32.const 0))

                (core instance $libc (instantiate $libc))
                (core type $start-func-ty (shared (func (param i32))))
                (alias core export $libc "mem" (core memory $mem))
                (alias core export $libc "__indirect_function_table"
                    (core table $table))
                (core func $thread-spawn-indirect
                    (canon thread.spawn-indirect $start-func-ty (table $table)))
                (core instance $i
                    (instantiate $m
                        (with "" (instance
                            (export "thread.spawn-indirect"
                                (func $thread-spawn-indirect))))
                        (with "libc" (instance
                            (export "mem" (memory $mem))
                            (export "__indirect_function_table" (table $table))))))

                (func (export "run") async (result u32)
                    (canon lift (core func $i "run") gc))
            )
            "#,
        )
    }

    fn thread_spawn_growable_table_component(engine: &Engine) -> Result<Component> {
        Component::new(
            engine,
            r#"
            (component
                (core module $libc
                    (memory (export "mem") 1 1 shared)
                    (table $table (export "__indirect_function_table") shared 1
                        (ref null (shared func))))

                (core module $m
                    (type $start-func-ty (shared (func (param i32))))
                    (type $spawn-indirect-ty
                        (shared (func (param i32 i32) (result i32))))
                    (type $run-ty (func (result i32)))
                    (import "" "thread.spawn-indirect"
                        (func $thread-spawn-indirect (type $spawn-indirect-ty)))
                    (import "libc" "mem" (memory $mem 1 1 shared))
                    (import "libc" "__indirect_function_table"
                        (table $table shared 1 (ref null (shared func))))
                    (func $thread-start (type $start-func-ty))
                    (elem (table $table) (i32.const 0)
                        (ref null (shared func)) (ref.func $thread-start))
                    (func (export "run") (type $run-ty)
                        i32.const 0))

                (core instance $libc (instantiate $libc))
                (core type $start-func-ty (shared (func (param i32))))
                (alias core export $libc "mem" (core memory $mem))
                (alias core export $libc "__indirect_function_table"
                    (core table $table))
                (core func $thread-spawn-indirect
                    (canon thread.spawn-indirect $start-func-ty (table $table)))
                (core instance $i
                    (instantiate $m
                        (with "" (instance
                            (export "thread.spawn-indirect"
                                (func $thread-spawn-indirect))))
                        (with "libc" (instance
                            (export "mem" (memory $mem))
                            (export "__indirect_function_table" (table $table))))))

                (func (export "run") async (result u32)
                    (canon lift (core func $i "run")))
            )
            "#,
        )
    }

    fn thread_spawn_defined_growable_table_start_component(engine: &Engine) -> Result<Component> {
        Component::new(
            engine,
            r#"
            (component
                (core module $libc
                    (memory (export "mem") 1 1 shared)
                    (table $table (export "__indirect_function_table") shared 1
                        (ref null (shared func)))
                    (type $start-func-ty (shared (func (param i32))))
                    (func $thread-start (type $start-func-ty))
                    (elem (table $table) (i32.const 0)
                        (ref null (shared func)) (ref.func $thread-start)))

                (core module $m
                    (type $spawn-indirect-ty
                        (shared (func (param i32 i32) (result i32))))
                    (type $run-ty (func (result i32)))
                    (import "" "thread.spawn-indirect"
                        (func $thread-spawn-indirect (type $spawn-indirect-ty)))
                    (import "libc" "mem" (memory $mem 1 1 shared))
                    (import "libc" "__indirect_function_table"
                        (table $table shared 1 (ref null (shared func))))
                    (func (export "run") (type $run-ty)
                        i32.const 0))

                (core instance $libc (instantiate $libc))
                (core type $start-func-ty (shared (func (param i32))))
                (alias core export $libc "mem" (core memory $mem))
                (alias core export $libc "__indirect_function_table"
                    (core table $table))
                (core func $thread-spawn-indirect
                    (canon thread.spawn-indirect $start-func-ty (table $table)))
                (core instance $i
                    (instantiate $m
                        (with "" (instance
                            (export "thread.spawn-indirect"
                                (func $thread-spawn-indirect))))
                        (with "libc" (instance
                            (export "mem" (memory $mem))
                            (export "__indirect_function_table" (table $table))))))

                (func (export "run") async (result u32)
                    (canon lift (core func $i "run")))
            )
            "#,
        )
    }

    fn thread_spawn_growable_table_owner_function_component(engine: &Engine) -> Result<Component> {
        Component::new(
            engine,
            r#"
            (component
                (core module $libc
                    (memory (export "mem") 1 1 shared)
                    (table $table (export "__indirect_function_table") shared 1
                        (ref null (shared func)))
                    (type $start-func-ty (shared (func (param i32))))
                    (func $helper (export "helper") (type $start-func-ty)
                        (drop (table.size $table))))

                (core module $m
                    (type $start-func-ty (shared (func (param i32))))
                    (type $spawn-indirect-ty
                        (shared (func (param i32 i32) (result i32))))
                    (type $run-ty (func (result i32)))
                    (import "" "thread.spawn-indirect"
                        (func $thread-spawn-indirect (type $spawn-indirect-ty)))
                    (import "libc" "mem" (memory $mem 1 1 shared))
                    (import "libc" "helper" (func $helper (type $start-func-ty)))
                    (import "libc" "__indirect_function_table"
                        (table $table shared 1 (ref null (shared func))))
                    (func $thread-start (type $start-func-ty)
                        (call $helper (local.get 0)))
                    (elem (table $table) (i32.const 0)
                        (ref null (shared func)) (ref.func $thread-start))
                    (func (export "run") (type $run-ty)
                        i32.const 0))

                (core instance $libc (instantiate $libc))
                (core type $start-func-ty (shared (func (param i32))))
                (alias core export $libc "mem" (core memory $mem))
                (alias core export $libc "helper" (core func $helper))
                (alias core export $libc "__indirect_function_table"
                    (core table $table))
                (core func $thread-spawn-indirect
                    (canon thread.spawn-indirect $start-func-ty (table $table)))
                (core instance $i
                    (instantiate $m
                        (with "" (instance
                            (export "thread.spawn-indirect"
                                (func $thread-spawn-indirect))))
                        (with "libc" (instance
                            (export "mem" (memory $mem))
                            (export "helper" (func $helper))
                            (export "__indirect_function_table" (table $table))))))

                (func (export "run") async (result u32)
                    (canon lift (core func $i "run")))
            )
            "#,
        )
    }

    fn thread_spawn_unrelated_growable_table_component(engine: &Engine) -> Result<Component> {
        Component::new(
            engine,
            r#"
            (component
                (core module $libc
                    (memory (export "mem") 1 1 shared)
                    (table $table (export "__indirect_function_table") shared 1 1
                        (ref null (shared func))))

                (core module $unused
                    (table (export "unused") shared 1
                        (ref null (shared func))))

                (core module $m
                    (type $start-func-ty (shared (func (param i32))))
                    (type $spawn-indirect-ty
                        (shared (func (param i32 i32) (result i32))))
                    (type $run-ty (func (result i32)))
                    (import "" "thread.spawn-indirect"
                        (func $thread-spawn-indirect (type $spawn-indirect-ty)))
                    (import "libc" "mem" (memory $mem 1 1 shared))
                    (import "libc" "__indirect_function_table"
                        (table $table shared 1 1 (ref null (shared func))))
                    (func $thread-start (type $start-func-ty))
                    (elem (table $table) (i32.const 0)
                        (ref null (shared func)) (ref.func $thread-start))
                    (func (export "run") (type $run-ty)
                        i32.const 0))

                (core instance $libc (instantiate $libc))
                (core instance $unused (instantiate $unused))
                (core type $start-func-ty (shared (func (param i32))))
                (alias core export $libc "mem" (core memory $mem))
                (alias core export $libc "__indirect_function_table"
                    (core table $table))
                (core func $thread-spawn-indirect
                    (canon thread.spawn-indirect $start-func-ty (table $table)))
                (core instance $i
                    (instantiate $m
                        (with "" (instance
                            (export "thread.spawn-indirect"
                                (func $thread-spawn-indirect))))
                        (with "libc" (instance
                            (export "mem" (memory $mem))
                            (export "__indirect_function_table" (table $table))))))

                (func (export "run") async (result u32)
                    (canon lift (core func $i "run")))
            )
            "#,
        )
    }

    fn thread_spawn_shared_global_component(engine: &Engine) -> Result<Component> {
        Component::new(
            engine,
            r#"
            (component
                (core module $state
                    (memory (export "mem") 1 1 shared)
                    (global (export "shared-global")
                        (shared mut i32) (i32.const 0))
                    (table $table (export "__indirect_function_table") shared 1 1
                        (ref null (shared func))))

                (core module $m
                    (type $start-func-ty (shared (func (param i32))))
                    (type $spawn-indirect-ty
                        (shared (func (param i32 i32) (result i32))))
                    (type $run-ty (func (result i32)))
                    (import "" "thread.spawn-indirect"
                        (func $thread-spawn-indirect (type $spawn-indirect-ty)))
                    (import "state" "mem" (memory $mem 1 1 shared))
                    (import "state" "shared-global"
                        (global $shared-global (shared mut i32)))
                    (import "state" "__indirect_function_table"
                        (table $table shared 1 1 (ref null (shared func))))
                    (func $thread-start (type $start-func-ty)
                        (i32.store (i32.const 0) (global.get $shared-global)))
                    (elem (table $table) (i32.const 0)
                        (ref null (shared func)) (ref.func $thread-start))
                    (func (export "run") (type $run-ty)
                        i32.const 0))

                (core instance $state (instantiate $state))
                (core type $start-func-ty (shared (func (param i32))))
                (alias core export $state "mem" (core memory $mem))
                (alias core export $state "shared-global" (core global $shared-global))
                (alias core export $state "__indirect_function_table"
                    (core table $table))
                (core func $thread-spawn-indirect
                    (canon thread.spawn-indirect $start-func-ty (table $table)))
                (core instance $i
                    (instantiate $m
                        (with "" (instance
                            (export "thread.spawn-indirect"
                                (func $thread-spawn-indirect))))
                        (with "state" (instance
                            (export "mem" (memory $mem))
                            (export "shared-global" (global $shared-global))
                            (export "__indirect_function_table" (table $table))))))

                (func (export "run") async (result u32)
                    (canon lift (core func $i "run")))
            )
            "#,
        )
    }

    fn thread_spawn_defined_shared_global_start_component(engine: &Engine) -> Result<Component> {
        Component::new(
            engine,
            r#"
            (component
                (core module $state
                    (memory (export "mem") 1 1 shared)
                    (global $shared-global (export "shared-global")
                        (shared mut i32) (i32.const 7))
                    (table $table (export "__indirect_function_table") shared 1 1
                        (ref null (shared func)))
                    (type $start-func-ty (shared (func (param i32))))
                    (func $thread-start (type $start-func-ty)
                        (i32.store (i32.const 0) (global.get $shared-global)))
                    (elem (table $table) (i32.const 0)
                        (ref null (shared func)) (ref.func $thread-start)))

                (core module $m
                    (type $spawn-indirect-ty
                        (shared (func (param i32 i32) (result i32))))
                    (type $run-ty (func (result i32)))
                    (import "" "thread.spawn-indirect"
                        (func $thread-spawn-indirect (type $spawn-indirect-ty)))
                    (func (export "run") (type $run-ty)
                        (call $thread-spawn-indirect
                            (i32.const 0)
                            (i32.const 0))))

                (core instance $state (instantiate $state))
                (core type $start-func-ty (shared (func (param i32))))
                (alias core export $state "__indirect_function_table"
                    (core table $table))
                (core func $thread-spawn-indirect
                    (canon thread.spawn-indirect $start-func-ty (table $table)))
                (core instance $i
                    (instantiate $m
                        (with "" (instance
                            (export "thread.spawn-indirect"
                                (func $thread-spawn-indirect))))))

                (func (export "run") async (result u32)
                    (canon lift (core func $i "run")))
            )
            "#,
        )
    }

    fn thread_spawn_defined_immutable_shared_global_start_component(
        engine: &Engine,
    ) -> Result<Component> {
        Component::new(
            engine,
            r#"
            (component
                (core module $state
                    (memory (export "mem") 1 1 shared)
                    (global $shared-global (export "shared-global")
                        (shared i32) (i32.const 7))
                    (table $table (export "__indirect_function_table") shared 1 1
                        (ref null (shared func)))
                    (type $start-func-ty (shared (func (param i32))))
                    (func $thread-start (type $start-func-ty)
                        (i32.store (i32.const 0) (global.get $shared-global)))
                    (elem (table $table) (i32.const 0)
                        (ref null (shared func)) (ref.func $thread-start)))

                (core module $m
                    (type $spawn-indirect-ty
                        (shared (func (param i32 i32) (result i32))))
                    (type $run-ty (func (result i32)))
                    (import "" "thread.spawn-indirect"
                        (func $thread-spawn-indirect (type $spawn-indirect-ty)))
                    (func (export "run") (type $run-ty)
                        (call $thread-spawn-indirect
                            (i32.const 0)
                            (i32.const 0))))

                (core instance $state (instantiate $state))
                (core type $start-func-ty (shared (func (param i32))))
                (alias core export $state "__indirect_function_table"
                    (core table $table))
                (core func $thread-spawn-indirect
                    (canon thread.spawn-indirect $start-func-ty (table $table)))
                (core instance $i
                    (instantiate $m
                        (with "" (instance
                            (export "thread.spawn-indirect"
                                (func $thread-spawn-indirect))))))

                (func (export "run") async (result u32)
                    (canon lift (core func $i "run")))
            )
            "#,
        )
    }

    fn thread_spawn_unshared_mutable_global_component(engine: &Engine) -> Result<Component> {
        Component::new(
            engine,
            r#"
            (component
                (core module $state
                    (memory (export "mem") 1 1 shared)
                    (global (export "unshared-global") (mut i32) (i32.const 0))
                    (table $table (export "__indirect_function_table") shared 1 1
                        (ref null (shared func))))

                (core module $m
                    (type $start-func-ty (shared (func (param i32))))
                    (type $spawn-indirect-ty
                        (shared (func (param i32 i32) (result i32))))
                    (type $run-ty (func (result i32)))
                    (import "" "thread.spawn-indirect"
                        (func $thread-spawn-indirect (type $spawn-indirect-ty)))
                    (import "state" "mem" (memory $mem 1 1 shared))
                    (import "state" "__indirect_function_table"
                        (table $table shared 1 1 (ref null (shared func))))
                    (func $thread-start (type $start-func-ty))
                    (elem (table $table) (i32.const 0)
                        (ref null (shared func)) (ref.func $thread-start))
                    (func (export "run") (type $run-ty)
                        i32.const 0))

                (core instance $state (instantiate $state))
                (core type $start-func-ty (shared (func (param i32))))
                (alias core export $state "mem" (core memory $mem))
                (alias core export $state "__indirect_function_table"
                    (core table $table))
                (core func $thread-spawn-indirect
                    (canon thread.spawn-indirect $start-func-ty (table $table)))
                (core instance $i
                    (instantiate $m
                        (with "" (instance
                            (export "thread.spawn-indirect"
                                (func $thread-spawn-indirect))))
                        (with "state" (instance
                            (export "mem" (memory $mem))
                            (export "__indirect_function_table" (table $table))))))

                (func (export "run") async (result u32)
                    (canon lift (core func $i "run")))
            )
            "#,
        )
    }

    fn engine() -> Result<Engine> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.wasm_component_model_async(true);
        config.wasm_component_model_threading(true);
        config.wasm_component_model_gc(true);
        config.wasm_threads(true);
        config.wasm_shared_everything_threads(true);
        config.wasm_function_references(true);
        config.wasm_gc(true);
        config.shared_memory(true);
        Engine::new(&config)
    }

    #[test]
    fn template_records_runtime_state_and_core_instance_gap() -> Result<()> {
        let engine = engine()?;
        let component = thread_template_component(&engine)?;

        let mut store = Store::new(&engine, ());
        let instance = Linker::new(&engine).instantiate(&mut store, &component)?;
        let template = instance.component_thread_template(&mut store);

        assert_eq!(
            template
                .instance_pre()
                .component()
                .env_component()
                .num_runtime_instances,
            2
        );
        assert_eq!(template.runtime_state().parent_core_instances().len(), 2);
        assert_eq!(template.runtime_state().runtime_memories().len(), 1);
        assert_eq!(template.runtime_state().runtime_tables().len(), 1);
        assert_eq!(template.runtime_state().core_shared_tables().len(), 1);
        assert_eq!(
            template.runtime_state().runtime_tables()[0].shareability(),
            ComponentThreadTableShareability::Shared,
        );
        assert_ne!(
            template.runtime_state().runtime_memories()[0].source_runtime_instance(),
            template.runtime_state().runtime_tables()[0].source_runtime_instance()
        );
        assert_ne!(
            template.runtime_state().runtime_memories()[0].source_parent_instance(),
            template.runtime_state().runtime_tables()[0].source_parent_instance()
        );
        assert_eq!(template.instantiated_core_modules(), 2);
        assert!(template.requires_core_instance_state_sharing());

        Ok(())
    }

    #[test]
    fn rebind_runtime_state_replaces_sibling_slots() -> Result<()> {
        let engine = engine()?;
        let component = thread_template_component(&engine)?;

        let mut store = Store::new(&engine, ());
        let parent = Linker::new(&engine).instantiate(&mut store, &component)?;
        let parent_template = parent.component_thread_template(&mut store);

        let sibling = parent_template.instance_pre().instantiate(&mut store)?;
        let sibling_before = sibling.component_thread_template(&mut store);

        assert_ne!(
            parent_template.runtime_state().runtime_memories()[0]
                .definition
                .as_ptr(),
            sibling_before.runtime_state().runtime_memories()[0]
                .definition
                .as_ptr(),
        );
        assert_ne!(
            parent_template.runtime_state().runtime_tables()[0]
                .import
                .from
                .as_ptr(),
            sibling_before.runtime_state().runtime_tables()[0]
                .import
                .from
                .as_ptr(),
        );

        unsafe {
            sibling.rebind_component_thread_runtime_state(&mut store, &parent_template);
        }

        let sibling_after = sibling.component_thread_template(&mut store);
        assert_eq!(
            parent_template.runtime_state().runtime_memories()[0]
                .definition
                .as_ptr(),
            sibling_after.runtime_state().runtime_memories()[0]
                .definition
                .as_ptr(),
        );
        assert_eq!(
            parent_template.runtime_state().runtime_tables()[0]
                .import
                .from
                .as_ptr(),
            sibling_after.runtime_state().runtime_tables()[0]
                .import
                .from
                .as_ptr(),
        );
        assert_eq!(
            parent_template.runtime_state().runtime_tables()[0]
                .import
                .vmctx
                .as_ptr(),
            sibling_after.runtime_state().runtime_tables()[0]
                .import
                .vmctx
                .as_ptr(),
        );

        Ok(())
    }

    #[test]
    fn rebind_validation_rejects_unshared_runtime_memory() -> Result<()> {
        let engine = engine()?;
        let component = thread_template_component(&engine)?;

        let mut store = Store::new(&engine, ());
        let instance = Linker::new(&engine).instantiate(&mut store, &component)?;
        let template = instance.component_thread_template(&mut store);

        let err = template.validate_rebindable_runtime_state().unwrap_err();
        let err = alloc::format!("{err:?}");
        assert!(err.contains("unshared runtime memory"), "{err:?}");

        Ok(())
    }

    #[test]
    fn unsafe_preemptive_validation_allows_start_table_only() -> Result<()> {
        let engine = engine()?;
        let component = thread_spawn_table_component(&engine)?;

        let mut store = Store::new(&engine, ());
        let instance = Linker::new(&engine).instantiate(&mut store, &component)?;
        let template = instance.component_thread_template(&mut store);

        assert_eq!(template.runtime_state().runtime_memories().len(), 0);
        assert_eq!(template.runtime_state().runtime_tables().len(), 1);
        assert_eq!(template.runtime_state().core_shared_memories().len(), 1);
        assert_eq!(template.runtime_state().core_shared_tables().len(), 1);
        assert!(template.spawn_plan_with_store_data_factory(None)?.is_some());
        let subset = template.unsafe_preemptive_shared_ownership_subset(
            Some(RuntimeTableIndex::from_u32(0)),
            None,
        )?;
        assert_eq!(subset.shared_core_memories(), 1);
        assert_eq!(subset.runtime_start_tables(), 1);
        assert_eq!(subset.fixed_core_shared_tables(), 1);
        assert_eq!(subset.growable_imported_runtime_start_tables(), 0);
        assert_eq!(subset.shared_global_definitions(), 0);
        assert_eq!(subset.direct_defined_mutable_shared_global_flushbacks(), 0);
        template.validate_unsafe_preemptive_spawn_indirect(RuntimeTableIndex::from_u32(0), None)?;

        let err = template
            .validate_unsafe_preemptive_spawn_indirect(RuntimeTableIndex::from_u32(1), None)
            .unwrap_err();
        let err = alloc::format!("{err:?}");
        assert!(
            err.contains("not the thread.spawn-indirect start table"),
            "{err:?}"
        );

        template.validate_rebindable_runtime_state()?;

        Ok(())
    }

    #[test]
    fn unsafe_preemptive_validation_rejects_growable_shared_table() -> Result<()> {
        let engine = engine()?;
        let component = thread_spawn_growable_table_component(&engine)?;

        let mut store = Store::new(&engine, ());
        let instance = Linker::new(&engine).instantiate(&mut store, &component)?;
        let template = instance.component_thread_template(&mut store);

        let err = template
            .validate_unsafe_preemptive_spawn_indirect(RuntimeTableIndex::from_u32(0), None)
            .unwrap_err();
        let err = alloc::format!("{err:?}");
        assert!(err.contains("growable shared core table"), "{err:?}");

        Ok(())
    }

    #[test]
    fn unsafe_preemptive_validation_allows_growable_imported_start_table() -> Result<()> {
        let engine = engine()?;
        let component = thread_spawn_growable_table_component(&engine)?;

        let mut store = Store::new(&engine, ());
        let instance = Linker::new(&engine).instantiate(&mut store, &component)?;
        let template = instance.component_thread_template(&mut store);
        let start_func_vmctx = {
            let store = store.as_context_mut().0;
            let component = store.component_instance(instance.id().instance());
            let core_instance = component.instance(RuntimeInstanceIndex::from_u32(1));
            VMOpaqueContext::from_vmcontext(store.instance(core_instance).vmctx())
        };

        assert_eq!(template.runtime_state().core_shared_tables().len(), 1);
        assert_eq!(template.runtime_state().runtime_tables().len(), 1);
        let subset = template.unsafe_preemptive_shared_ownership_subset(
            Some(RuntimeTableIndex::from_u32(0)),
            Some(start_func_vmctx),
        )?;
        assert_eq!(subset.fixed_core_shared_tables(), 0);
        assert_eq!(subset.growable_imported_runtime_start_tables(), 1);
        template.validate_unsafe_preemptive_spawn_indirect(
            RuntimeTableIndex::from_u32(0),
            Some(start_func_vmctx),
        )?;

        Ok(())
    }

    #[test]
    fn unsafe_preemptive_validation_rejects_unowned_growable_shared_table() -> Result<()> {
        let engine = engine()?;
        let component = thread_spawn_unrelated_growable_table_component(&engine)?;

        let mut store = Store::new(&engine, ());
        let instance = Linker::new(&engine).instantiate(&mut store, &component)?;
        let template = instance.component_thread_template(&mut store);
        let start_func_vmctx = {
            let store = store.as_context_mut().0;
            let component = store.component_instance(instance.id().instance());
            let core_instance = component.instance(RuntimeInstanceIndex::from_u32(2));
            VMOpaqueContext::from_vmcontext(store.instance(core_instance).vmctx())
        };

        let err = template
            .validate_unsafe_preemptive_spawn_indirect(
                RuntimeTableIndex::from_u32(0),
                Some(start_func_vmctx),
            )
            .unwrap_err();
        let err = alloc::format!("{err:?}");
        assert!(
            err.contains("outside the Vibe shared ownership subset"),
            "{err:?}"
        );

        Ok(())
    }

    #[test]
    fn unsafe_preemptive_validation_rejects_growable_table_owner_functions() -> Result<()> {
        let engine = engine()?;
        let component = thread_spawn_growable_table_owner_function_component(&engine)?;

        let mut store = Store::new(&engine, ());
        let instance = Linker::new(&engine).instantiate(&mut store, &component)?;
        let template = instance.component_thread_template(&mut store);
        let start_func_vmctx = {
            let store = store.as_context_mut().0;
            let component = store.component_instance(instance.id().instance());
            let core_instance = component.instance(RuntimeInstanceIndex::from_u32(1));
            VMOpaqueContext::from_vmcontext(store.instance(core_instance).vmctx())
        };

        let err = template
            .validate_unsafe_preemptive_spawn_indirect(
                RuntimeTableIndex::from_u32(0),
                Some(start_func_vmctx),
            )
            .unwrap_err();
        let err = alloc::format!("{err:?}");
        assert!(err.contains("growable shared table owner"), "{err:?}");

        Ok(())
    }

    #[test]
    fn unsafe_preemptive_validation_rejects_growable_defined_table_start() -> Result<()> {
        let engine = engine()?;
        let component = thread_spawn_defined_growable_table_start_component(&engine)?;

        let mut store = Store::new(&engine, ());
        let instance = Linker::new(&engine).instantiate(&mut store, &component)?;
        let template = instance.component_thread_template(&mut store);
        let start_func_vmctx = {
            let store = store.as_context_mut().0;
            let component = store.component_instance(instance.id().instance());
            let core_instance = component.instance(RuntimeInstanceIndex::from_u32(0));
            VMOpaqueContext::from_vmcontext(store.instance(core_instance).vmctx())
        };

        let err = template
            .validate_unsafe_preemptive_spawn_indirect(
                RuntimeTableIndex::from_u32(0),
                Some(start_func_vmctx),
            )
            .unwrap_err();
        let err = alloc::format!("{err:?}");
        assert!(
            err.contains("direct defined growable shared table"),
            "{err:?}"
        );

        Ok(())
    }

    #[test]
    fn unsafe_preemptive_validation_rejects_component_resources() -> Result<()> {
        let engine = engine()?;
        let component = thread_spawn_resource_component(&engine)?;

        let mut store = Store::new(&engine, ());
        let instance = Linker::new(&engine).instantiate(&mut store, &component)?;
        let template = instance.component_thread_template(&mut store);

        assert_eq!(
            template
                .instance_pre()
                .component()
                .env_component()
                .num_resources,
            1
        );
        let err = template
            .validate_unsafe_preemptive_spawn_indirect(RuntimeTableIndex::from_u32(0), None)
            .unwrap_err();
        let err = alloc::format!("{err:?}");
        assert!(err.contains("component resources"), "{err:?}");

        Ok(())
    }

    #[test]
    fn unsafe_preemptive_validation_rejects_component_gc_options() -> Result<()> {
        let engine = engine()?;
        let component = thread_spawn_gc_component(&engine)?;

        let mut store = Store::new(&engine, ());
        let instance = Linker::new(&engine).instantiate(&mut store, &component)?;
        let template = instance.component_thread_template(&mut store);

        let err = template
            .validate_unsafe_preemptive_spawn_indirect(RuntimeTableIndex::from_u32(0), None)
            .unwrap_err();
        let err = alloc::format!("{err:?}");
        assert!(err.contains("component-model GC"), "{err:?}");

        Ok(())
    }

    #[test]
    fn unsafe_preemptive_validation_allows_defined_mutable_shared_global_start() -> Result<()> {
        let engine = engine()?;
        let component = thread_spawn_defined_shared_global_start_component(&engine)?;

        let mut store = Store::new(&engine, ());
        let instance = Linker::new(&engine).instantiate(&mut store, &component)?;
        let template = instance.component_thread_template(&mut store);
        let start_func_vmctx = {
            let store = store.as_context_mut().0;
            let component = store.component_instance(instance.id().instance());
            let core_instance = component.instance(RuntimeInstanceIndex::from_u32(0));
            VMOpaqueContext::from_vmcontext(store.instance(core_instance).vmctx())
        };

        assert_eq!(template.runtime_state().core_shared_globals().len(), 1);
        let subset = template.unsafe_preemptive_shared_ownership_subset(
            Some(RuntimeTableIndex::from_u32(0)),
            Some(start_func_vmctx),
        )?;
        assert_eq!(subset.shared_global_definitions(), 1);
        assert_eq!(subset.direct_defined_mutable_shared_global_flushbacks(), 1);
        template.validate_unsafe_preemptive_spawn_indirect(
            RuntimeTableIndex::from_u32(0),
            Some(start_func_vmctx),
        )?;

        Ok(())
    }

    #[test]
    fn unsafe_preemptive_validation_allows_defined_immutable_shared_global_start() -> Result<()> {
        let engine = engine()?;
        let component = thread_spawn_defined_immutable_shared_global_start_component(&engine)?;

        let mut store = Store::new(&engine, ());
        let instance = Linker::new(&engine).instantiate(&mut store, &component)?;
        let template = instance.component_thread_template(&mut store);
        let start_func_vmctx = {
            let store = store.as_context_mut().0;
            let component = store.component_instance(instance.id().instance());
            let core_instance = component.instance(RuntimeInstanceIndex::from_u32(0));
            VMOpaqueContext::from_vmcontext(store.instance(core_instance).vmctx())
        };

        assert_eq!(template.runtime_state().core_shared_globals().len(), 1);
        template.validate_unsafe_preemptive_spawn_indirect(
            RuntimeTableIndex::from_u32(0),
            Some(start_func_vmctx),
        )?;

        Ok(())
    }

    #[test]
    fn spawn_plan_rebinds_child_core_shared_memory() -> Result<()> {
        let engine = engine()?;
        let component = thread_spawn_table_component(&engine)?;

        let mut store = Store::new(&engine, ());
        let parent = Linker::new(&engine).instantiate(&mut store, &component)?;
        let parent_template = parent.component_thread_template(&mut store);
        let plan = parent_template
            .spawn_plan_with_store_data_factory(None)?
            .unwrap();

        let child = plan.instance_pre().instantiate(&mut store)?;
        let child_before = child.component_thread_template(&mut store);
        let child_import_before = {
            let store = store.as_context_mut().0;
            let component = store.component_instance(child.id().instance());
            let core_instance = component.instance(RuntimeInstanceIndex::from_u32(1));
            store
                .instance(core_instance)
                .component_thread_imported_memory(MemoryIndex::new(0))
                .from
                .as_non_null()
        };
        assert_ne!(
            parent_template.runtime_state().core_shared_memories()[0]
                .definition()
                .as_ptr(),
            child_before.runtime_state().core_shared_memories()[0]
                .definition()
                .as_ptr(),
        );
        assert_ne!(
            parent_template.runtime_state().core_shared_memories()[0]
                .shared_memory
                .vmmemory_ptr()
                .as_ptr(),
            child_before.runtime_state().core_shared_memories()[0]
                .shared_memory
                .vmmemory_ptr()
                .as_ptr(),
        );
        assert_ne!(
            parent_template.runtime_state().core_shared_memories()[0]
                .definition()
                .as_ptr(),
            child_import_before.as_ptr(),
        );

        unsafe {
            plan.rebind_core_shared_memories_to(store.as_context_mut().0, child.id().instance());
        }

        let child_after = child.component_thread_template(&mut store);
        let child_import_after = {
            let store = store.as_context_mut().0;
            let component = store.component_instance(child.id().instance());
            let core_instance = component.instance(RuntimeInstanceIndex::from_u32(1));
            store
                .instance(core_instance)
                .component_thread_imported_memory(MemoryIndex::new(0))
                .from
                .as_non_null()
        };
        assert_eq!(
            parent_template.runtime_state().core_shared_memories()[0]
                .definition()
                .as_ptr(),
            child_after.runtime_state().core_shared_memories()[0]
                .definition()
                .as_ptr(),
        );
        assert_eq!(
            parent_template.runtime_state().core_shared_memories()[0]
                .shared_memory
                .vmmemory_ptr()
                .as_ptr(),
            child_after.runtime_state().core_shared_memories()[0]
                .shared_memory
                .vmmemory_ptr()
                .as_ptr(),
        );
        assert_eq!(
            parent_template.runtime_state().core_shared_memories()[0]
                .definition()
                .as_ptr(),
            child_import_after.as_ptr(),
        );

        Ok(())
    }

    #[test]
    fn spawn_plan_rebinds_child_core_shared_table_and_runtime_slot() -> Result<()> {
        let engine = engine()?;
        let component = thread_spawn_table_component(&engine)?;

        let mut store = Store::new(&engine, ());
        let parent = Linker::new(&engine).instantiate(&mut store, &component)?;
        let parent_template = parent.component_thread_template(&mut store);
        let plan = parent_template
            .spawn_plan_with_store_data_factory(None)?
            .unwrap();

        let child = plan.instance_pre().instantiate(&mut store)?;
        let child_before = child.component_thread_template(&mut store);
        let child_import_before = {
            let store = store.as_context_mut().0;
            let component = store.component_instance(child.id().instance());
            let core_instance = component.instance(RuntimeInstanceIndex::from_u32(1));
            store
                .instance(core_instance)
                .component_thread_imported_table(TableIndex::new(0))
        };
        assert_ne!(
            parent_template.runtime_state().core_shared_tables()[0]
                .definition_value()
                .base
                .as_ptr(),
            child_before.runtime_state().core_shared_tables()[0]
                .definition_value()
                .base
                .as_ptr(),
        );
        assert_ne!(
            parent_template.runtime_state().core_shared_tables()[0]
                .definition()
                .as_ptr(),
            child_import_before.from.as_ptr(),
        );
        assert_ne!(
            parent_template.runtime_state().runtime_tables()[0]
                .import
                .from
                .as_ptr(),
            child_before.runtime_state().runtime_tables()[0]
                .import
                .from
                .as_ptr(),
        );

        unsafe {
            plan.rebind_core_shared_tables_to(store.as_context_mut().0, child.id().instance());
            plan.rebind_shared_runtime_tables_to(store.as_context_mut().0, child.id().instance());
        }

        let child_after = child.component_thread_template(&mut store);
        let child_import_after = {
            let store = store.as_context_mut().0;
            let component = store.component_instance(child.id().instance());
            let core_instance = component.instance(RuntimeInstanceIndex::from_u32(1));
            store
                .instance(core_instance)
                .component_thread_imported_table(TableIndex::new(0))
        };
        assert_eq!(
            parent_template.runtime_state().core_shared_tables()[0]
                .definition_value()
                .base
                .as_ptr(),
            child_after.runtime_state().core_shared_tables()[0]
                .definition_value()
                .base
                .as_ptr(),
        );
        assert_eq!(
            parent_template.runtime_state().core_shared_tables()[0]
                .definition()
                .as_ptr(),
            child_import_after.from.as_ptr(),
        );
        assert_eq!(
            parent_template.runtime_state().runtime_tables()[0]
                .import
                .from
                .as_ptr(),
            child_after.runtime_state().runtime_tables()[0]
                .import
                .from
                .as_ptr(),
        );
        assert_eq!(
            parent_template.runtime_state().runtime_tables()[0]
                .import
                .vmctx
                .as_ptr(),
            child_after.runtime_state().runtime_tables()[0]
                .import
                .vmctx
                .as_ptr(),
        );

        Ok(())
    }

    #[test]
    fn spawn_plan_rebinds_child_core_shared_global_import() -> Result<()> {
        let engine = engine()?;
        let component = thread_spawn_shared_global_component(&engine)?;

        let mut store = Store::new(&engine, ());
        let parent = Linker::new(&engine).instantiate(&mut store, &component)?;
        let parent_template = parent.component_thread_template(&mut store);
        let plan = parent_template
            .spawn_plan_with_store_data_factory(None)?
            .unwrap();

        let child = plan.instance_pre().instantiate(&mut store)?;
        let child_template = child.component_thread_template(&mut store);
        let child_import_before = {
            let store = store.as_context_mut().0;
            let component = store.component_instance(child.id().instance());
            let core_instance = component.instance(RuntimeInstanceIndex::from_u32(1));
            store
                .instance(core_instance)
                .component_thread_imported_global(GlobalIndex::new(0))
        };
        assert_eq!(
            parent_template.runtime_state().core_shared_globals().len(),
            1
        );
        assert_eq!(
            child_template.runtime_state().core_shared_globals().len(),
            1
        );
        assert_ne!(
            parent_template.runtime_state().core_shared_globals()[0]
                .definition()
                .as_ptr(),
            child_template.runtime_state().core_shared_globals()[0]
                .definition()
                .as_ptr(),
        );
        assert_ne!(
            parent_template.runtime_state().core_shared_globals()[0]
                .definition()
                .as_ptr(),
            child_import_before.from.as_ptr(),
        );
        {
            let store = store.as_context_mut().0;
            let component = store.component_instance(parent.id().instance());
            let core_instance = component.instance(RuntimeInstanceIndex::from_u32(0));
            let mut value = VMGlobalDefinition::new();
            unsafe {
                *value.as_i32_mut() = 37;
                store
                    .instance_mut(core_instance)
                    .component_thread_write_defined_global(DefinedGlobalIndex::new(0), value);
            }
        }

        unsafe {
            plan.rebind_core_shared_globals_to(store.as_context_mut().0, child.id().instance());
        }

        let child_import_after = {
            let store = store.as_context_mut().0;
            let component = store.component_instance(child.id().instance());
            let core_instance = component.instance(RuntimeInstanceIndex::from_u32(1));
            store
                .instance(core_instance)
                .component_thread_imported_global(GlobalIndex::new(0))
        };
        assert_eq!(
            parent_template.runtime_state().core_shared_globals()[0]
                .definition()
                .as_ptr(),
            child_import_after.from.as_ptr(),
        );
        let child_definition_after = {
            let store = store.as_context_mut().0;
            let component = store.component_instance(child.id().instance());
            let core_instance = component.instance(RuntimeInstanceIndex::from_u32(0));
            store
                .instance(core_instance)
                .component_thread_read_defined_global(DefinedGlobalIndex::new(0))
        };
        assert_eq!(unsafe { *child_definition_after.as_i32() }, 37);

        Ok(())
    }

    #[test]
    fn unsafe_preemptive_validation_rejects_unshared_mutable_global() -> Result<()> {
        let engine = engine()?;
        let component = thread_spawn_unshared_mutable_global_component(&engine)?;

        let mut store = Store::new(&engine, ());
        let instance = Linker::new(&engine).instantiate(&mut store, &component)?;
        let template = instance.component_thread_template(&mut store);

        assert_eq!(
            template
                .runtime_state()
                .core_unshared_mutable_globals()
                .len(),
            1
        );
        let err = template
            .validate_unsafe_preemptive_spawn_indirect(RuntimeTableIndex::from_u32(0), None)
            .unwrap_err();
        let err = alloc::format!("{err:?}");
        assert!(err.contains("mutable unshared global"), "{err:?}");

        Ok(())
    }

    #[test]
    fn spawn_plan_uses_non_unit_store_data_factory() -> Result<()> {
        #[derive(Debug, Eq, PartialEq)]
        struct ThreadStoreData {
            value: u32,
        }

        let engine = engine()?;
        let component = thread_spawn_table_component(&engine)?;

        let mut store = Store::new(&engine, ThreadStoreData { value: 1 });
        store.set_unsafe_component_thread_store_data_factory(|| ThreadStoreData { value: 42 });

        let parent = Linker::new(&engine).instantiate(&mut store, &component)?;
        let template = parent.component_thread_template(&mut store);
        let factory = store
            .as_context_mut()
            .0
            .component_thread_store_data_factory();
        let plan = template
            .spawn_plan_with_store_data_factory(factory)?
            .unwrap();

        let child_store = plan.new_store();
        assert_eq!(child_store.data().value, 42);

        Ok(())
    }

    #[test]
    fn spawn_plan_rejects_non_unit_store_data_without_factory() -> Result<()> {
        struct ThreadStoreData;

        let engine = engine()?;
        let component = thread_spawn_table_component(&engine)?;

        let mut store = Store::new(&engine, ThreadStoreData);
        let parent = Linker::new(&engine).instantiate(&mut store, &component)?;
        let template = parent.component_thread_template(&mut store);

        let err = match template.spawn_plan_with_store_data_factory(None) {
            Ok(_) => panic!("expected non-unit store data without a factory to be rejected"),
            Err(err) => err,
        };
        let err = alloc::format!("{err:?}");
        assert!(
            err.contains("requires a per-thread store-data factory"),
            "{err:?}"
        );

        Ok(())
    }
}
