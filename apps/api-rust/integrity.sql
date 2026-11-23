BEGIN TRANSACTION READ ONLY;
SELECT 'duplicate_active_tools' AS check_name, count(*) AS findings FROM (SELECT tool_id FROM tool_placements WHERE removed_at IS NULL GROUP BY tool_id HAVING count(*) > 1) x
UNION ALL SELECT 'duplicate_active_compartments', count(*) FROM (SELECT compartment_id FROM tool_placements WHERE removed_at IS NULL GROUP BY compartment_id HAVING count(*) > 1) x
UNION ALL SELECT 'invalid_placement_hierarchy', count(*) FROM tool_placements p JOIN compartments c ON c.id=p.compartment_id JOIN lockers l ON l.id=p.locker_id WHERE c.locker_id<>p.locker_id OR l.branch_id<>p.branch_id
UNION ALL SELECT 'active_loan_with_active_placement', count(*) FROM loans l JOIN tool_placements p ON p.tool_id=l.tool_id AND p.removed_at IS NULL WHERE l.status='ACTIVE'
UNION ALL SELECT 'multiple_open_loans', count(*) FROM (SELECT tool_id FROM loans WHERE status IN ('ACTIVE','CHECKOUT_PENDING','RETURN_PENDING') GROUP BY tool_id HAVING count(*) > 1) x
UNION ALL SELECT 'pending_or_failed_operations', count(*) FROM locker_operations WHERE status IN ('PENDING','FAILED');
ROLLBACK;
