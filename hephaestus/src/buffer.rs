use core::slice;
use std::ffi::c_void;

use ash::{
    prelude::VkResult,
    vk::{
        self, BufferCreateInfo, BufferUsageFlags, MemoryAllocateInfo, MemoryMapFlags,
        MemoryPropertyFlags, SharingMode,
    },
};

use crate::{
    command::Region,
    task::{SubmitInfo, Task},
    Context, Device,
};

pub trait Buffer {
    fn buffer(&self) -> vk::Buffer;
    fn memory(&self) -> vk::DeviceMemory;
}

pub struct Dynamic {
    pub handle: vk::Buffer,
    pub memory: vk::DeviceMemory,
}

impl Dynamic {
    pub fn new(
        ctx: &Context,
        size: usize,
        usage: BufferUsageFlags,
    ) -> VkResult<Self> {
        let create_info = BufferCreateInfo::builder()
            .size(size as u64)
            .usage(usage)
            .sharing_mode(SharingMode::EXCLUSIVE);
        let handle = unsafe { ctx.device.create_buffer(&create_info, None)? };

        let requirements = unsafe { ctx.device.get_buffer_memory_requirements(handle) };
        let properties =
            unsafe { ctx.instance.get_physical_device_memory_properties(ctx.device.physical.handle) };

        let type_index = properties
            .memory_types
            .iter()
            .enumerate()
            .position(|(i, ty)| {
                (requirements.memory_type_bits & (1 << i)) != 0
                    && ty.property_flags.contains(
                        MemoryPropertyFlags::HOST_VISIBLE | MemoryPropertyFlags::HOST_COHERENT,
                    )
            })
            .expect("No memory type found that satisfies requirements");

        let alloc_info = MemoryAllocateInfo::builder()
            .allocation_size(requirements.size)
            .memory_type_index(type_index as u32);
        let memory = unsafe { ctx.device.allocate_memory(&alloc_info, None)? };
        unsafe { ctx.device.bind_buffer_memory(handle, memory, 0)? };

        Ok(Self { handle, memory })
    }

    pub fn write(&self, device: &Device, data: &[u8]) -> VkResult<()> {
        let memory: *mut c_void = unsafe {
            device.map_memory(self.memory, 0, data.len() as u64, MemoryMapFlags::default())?
        };
        let memory: *mut u8 = memory.cast();
        unsafe { slice::from_raw_parts_mut(memory, data.len()).copy_from_slice(data) };
        unsafe { device.unmap_memory(self.memory) };

        Ok(())
    }

    pub fn destroy(self, device: &Device) {
        unsafe { device.destroy_buffer(self.handle, None) }
        unsafe { device.free_memory(self.memory, None) }
    }
}

impl Buffer for Dynamic {
    fn buffer(&self) -> vk::Buffer {
        self.handle
    }

    fn memory(&self) -> vk::DeviceMemory {
        self.memory
    }
}

pub struct Static {
    pub handle: vk::Buffer,
    pub memory: vk::DeviceMemory,
}

impl Static {
    pub fn new(ctx: &Context, data: &[u8], usage: BufferUsageFlags) -> VkResult<Self> {
        let size = data.len();
        let create_info = BufferCreateInfo::builder()
            .size(size as u64)
            .usage(usage | BufferUsageFlags::TRANSFER_DST)
            .sharing_mode(SharingMode::EXCLUSIVE);
        let handle = unsafe { ctx.device.create_buffer(&create_info, None)? };

        let requirements = unsafe { ctx.device.get_buffer_memory_requirements(handle) };
        let properties = unsafe {
            ctx.instance
                .get_physical_device_memory_properties(ctx.device.physical.handle)
        };

        let type_index = properties
            .memory_types
            .iter()
            .enumerate()
            .position(|(i, ty)| {
                (requirements.memory_type_bits & (1 << i)) != 0
                    && ty
                        .property_flags
                        .contains(MemoryPropertyFlags::DEVICE_LOCAL)
            })
            .expect("No memory type found that satisfies requirements");

        let alloc_info = MemoryAllocateInfo::builder()
            .allocation_size(requirements.size)
            .memory_type_index(type_index as u32);
        let memory = unsafe { ctx.device.allocate_memory(&alloc_info, None)? };
        unsafe { ctx.device.bind_buffer_memory(handle, memory, 0)? };

        let staging = Dynamic::new(
            ctx,
            size,
            BufferUsageFlags::TRANSFER_SRC,
        )?;
        staging.write(&ctx.device, data)?;

        let buffer = Self { handle, memory };

        let cmd = ctx
            .command_pool
            .alloc(&ctx.device)?
            .begin(&ctx.device)?
            .copy_buffer(
                &staging,
                &buffer,
                Region {
                    from_offset: 0,
                    to_offset: 0,
                    size,
                },
            )
            .end()?;

        let mut task = Task::new();
        let fence = task.fence(&ctx.device)?;
        task.submit(SubmitInfo {
            cmd: &cmd,
            fence: fence.clone(),
            device: &ctx.device,
            queue: &ctx.device.queues.graphics,
            wait: &[],
            signal: &[],
        })?;
        fence.wait(&ctx.device)?;
        task.destroy(&ctx.device);

        staging.destroy(&ctx.device);

        Ok(buffer)
    }

    pub fn destroy(self, device: &Device) {
        unsafe { device.destroy_buffer(self.handle, None) }
        unsafe { device.free_memory(self.memory, None) }
    }
}

impl Buffer for Static {
    fn buffer(&self) -> vk::Buffer {
        self.handle
    }

    fn memory(&self) -> vk::DeviceMemory {
        self.memory
    }
}
