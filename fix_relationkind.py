import os

files = [
    'src/engine.rs', 'src/executor.rs', 'src/machine.rs',
    'src/wal.rs', 'src/instruction.rs', 'src/instruction_codec.rs',
    'src/module.rs', 'src/program.rs'
]

for f in files:
    if os.path.exists(f):
        with open(f, 'r') as fh:
            content = fh.read()
        
        # Replace RelationKind with LinkType
        content = content.replace('RelationKind', 'LinkType')
        content = content.replace('relation_kind', 'link_type')
        # Replace enum values
        content = content.replace('CapabilityDelegation', 'DependsOn')
        content = content.replace('ContractDependency', 'DependsOn')
        content = content.replace('EffectPropagation', 'References')
        # Fix field name
        content = content.replace('.relation', '.link_type')
        
        with open(f, 'w') as fh:
            fh.write(content)
        
        print(f'Fixed {f}')

print('Done')
