use crate::{
    object::{HeapObject, ObjectPointer, StackObject, UnallocatedObject}, InterpreterContext, InterpreterError, InterpreterErrorKind, InterpreterResult
};

pub trait InterpreterStackAlloc: Sized {
    fn stack_alloc(self, interpreter: &mut InterpreterContext) -> InterpreterResult<StackObject>;
}

impl InterpreterStackAlloc for ObjectPointer {
    fn stack_alloc(self, interpreter: &mut InterpreterContext) -> InterpreterResult<StackObject> {
        match self {
            ObjectPointer::Null => Err(InterpreterError::new(InterpreterErrorKind::CannotAllocateNull)),
            p => Ok(StackObject::Ref(p)),
        }
    }
}

impl InterpreterStackAlloc for UnallocatedObject {
    fn stack_alloc(self, interpreter: &mut InterpreterContext) -> InterpreterResult<StackObject> {
        match self {
            UnallocatedObject::Value(v) => Ok(StackObject::Value(v)),
            o => Ok(StackObject::Ref(o.heap_alloc(interpreter)?))
        }
    }
}

impl InterpreterStackAlloc for HeapObject {
    fn stack_alloc(self, interpreter: &mut InterpreterContext) -> InterpreterResult<StackObject> {
        Ok(StackObject::Ref(self.heap_alloc(interpreter)?))
    }
}

pub trait InterpreterHeapAlloc: Sized {
    fn heap_alloc_named(
        self,
        ident: &str,
        interpreter: &mut InterpreterContext,
    ) -> InterpreterResult<ObjectPointer> {
        let p = self.heap_alloc(interpreter)?;
        interpreter.ident_mapping.insert(ident.to_string(), p);
        Ok(p)
    }

    fn heap_alloc(self, interpreter: &mut InterpreterContext) -> InterpreterResult<ObjectPointer>;
}

impl InterpreterHeapAlloc for HeapObject {
    fn heap_alloc(self, interpreter: &mut InterpreterContext) -> InterpreterResult<ObjectPointer> {
        let id = interpreter
            .heap
            .iter()
            .enumerate()
            .find_map(|(i, o)| o.is_none().then_some(i))
            .unwrap_or(interpreter.heap.len());

        if id >= interpreter.heap.len() {
            let extend = ((interpreter.heap.len())..=id + 1)
                .map(|_| None);
            interpreter.heap.extend(extend);
        }

        let _ = interpreter.heap.get_mut(id).as_mut().unwrap().insert(self);
        Ok(ObjectPointer::Heap(id))
    }
}

impl InterpreterHeapAlloc for StackObject {
    fn heap_alloc(self, interpreter: &mut InterpreterContext) -> InterpreterResult<ObjectPointer> {
        match self {
            StackObject::Value(v) => HeapObject::Value(v).heap_alloc(interpreter),
            StackObject::Ref(p) => Ok(p),
        }
    }
}

impl InterpreterHeapAlloc for UnallocatedObject {
    fn heap_alloc(self, interpreter: &mut InterpreterContext) -> InterpreterResult<ObjectPointer> {
        match self {
            UnallocatedObject::Value(v) => HeapObject::Value(v),
            UnallocatedObject::Func(f) => HeapObject::Func(f),
            UnallocatedObject::String(s) => HeapObject::String(s),
            UnallocatedObject::List(head, tail) => {
                let head = head.heap_alloc(interpreter)?;
                let tail = tail.heap_alloc(interpreter)?;
                HeapObject::List(head, tail)
            }
            UnallocatedObject::Null => return Err(InterpreterError::new(InterpreterErrorKind::CannotAllocateNull)),
        }
        .heap_alloc(interpreter)
    }
}
