    #[test]
    fn large_payload_measurements_justify_decode_defaults() {
        use crate::io::binary::BinaryReader;
        use std::time::Instant;

        // Representative large-file shapes must decode inside the product
        // defaults (256 MB allocation, 16 M array elements, 64 MB strings,
        // depth 256). Timings are reported for the record; success (not
        // speed) is asserted.
        let limits = super::ObjectDecodeLimits::default();

        // 2M-element int vector (8 MB payload).
        {
            let layout = element_layout(vec![
                layout_node(0, "m_Large", "vector"),
                layout_node(1, "Array", "Array"),
                layout_node(2, "size", "int"),
                layout_node(2, "data", "int"),
            ]);
            let count = 2_000_000usize;
            let mut payload = Vec::with_capacity(4 + count * 4);
            payload.extend_from_slice(&(count as i32).to_le_bytes());
            payload.extend(std::iter::repeat_n(0u8, count * 4));
            let mut budget = super::DecodeBudget {
                limits: &limits,
                allocated_bytes: 0,
                managed_type_references: &[],
                managed_reference_type_resolver: None,
                stream_offset: 0,
                managed_registry_depth: 0,
            };
            let mut reader = BinaryReader::new(&payload, Endian::Little);
            let start = Instant::now();
            let field = super::decode_field(&mut reader, &layout, 0, false, 0, "", 0, &mut budget)
                .expect("2M int vector decodes");
            let elapsed = start.elapsed();
            match field.value {
                SerializedValue::Array(elements) => assert_eq!(elements.len(), count),
                value => panic!("expected array, got {value:?}"),
            }
            println!(
                "LIMITS_MEASURE int-vector-2M elapsed_ms={}",
                elapsed.as_millis()
            );
        }

        // 8 MB string (length-prefixed, 4-aligned).
        {
            let layout = element_layout(vec![
                layout_node(0, "m_Big", "string"),
                layout_node(1, "Array", "Array"),
                layout_node(2, "size", "int"),
                layout_node(2, "data", "char"),
            ]);
            let length = 8_000_000usize;
            let mut payload = Vec::with_capacity(4 + length + 4);
            payload.extend_from_slice(&(length as i32).to_le_bytes());
            payload.extend(std::iter::repeat_n(b'x', length));
            while payload.len() % 4 != 0 {
                payload.push(0);
            }
            let mut budget = super::DecodeBudget {
                limits: &limits,
                allocated_bytes: 0,
                managed_type_references: &[],
                managed_reference_type_resolver: None,
                stream_offset: 0,
                managed_registry_depth: 0,
            };
            let mut reader = BinaryReader::new(&payload, Endian::Little);
            let start = Instant::now();
            let field = super::decode_field(&mut reader, &layout, 0, false, 0, "", 0, &mut budget)
                .expect("8MB string decodes");
            let elapsed = start.elapsed();
            match field.value {
                SerializedValue::StringBytes(bytes) => assert_eq!(bytes.len(), length),
                value => panic!("expected string, got {value:?}"),
            }
            println!(
                "LIMITS_MEASURE string-8M elapsed_ms={}",
                elapsed.as_millis()
            );
        }

        // 65-level struct chain (root plus 64 nested, well under the 256 depth cap).
        {
            let mut nodes = vec![layout_node(0, "m_Root", "S0")];
            for depth in 1..=64 {
                nodes.push(layout_node(
                    depth as u8,
                    Box::leak(format!("m_D{depth}").into_boxed_str()),
                    Box::leak(format!("S{depth}").into_boxed_str()),
                ));
            }
            nodes.push(layout_node(65, "m_Value", "int"));
            let layout = element_layout(nodes);
            let payload = 12345i32.to_le_bytes().to_vec();
            let mut budget = super::DecodeBudget {
                limits: &limits,
                allocated_bytes: 0,
                managed_type_references: &[],
                managed_reference_type_resolver: None,
                stream_offset: 0,
                managed_registry_depth: 0,
            };
            let mut reader = BinaryReader::new(&payload, Endian::Little);
            let start = Instant::now();
            let field = super::decode_field(&mut reader, &layout, 0, false, 0, "", 0, &mut budget)
                .expect("64-deep nesting decodes");
            let elapsed = start.elapsed();
            let mut scope = &field.value;
            for _ in 0..65 {
                let fields = scope.struct_fields().expect("nested struct");
                assert_eq!(fields.len(), 1);
                scope = &fields[0].value;
            }
            assert_eq!(scope, &SerializedValue::I32(12345));
            println!(
                "LIMITS_MEASURE nesting-64 elapsed_ms={}",
                elapsed.as_millis()
            );
        }
    }
