import React, { useEffect, useState } from 'react';
import Button from '@mui/material/Button';
import Paper from '@mui/material/Paper';
import Box from '@mui/material/Box';
import { TableBody, TableCell, TableHead, TableRow, TextField, Typography } from '@mui/material';
import Table from '@mui/material/Table';
import { useWalletContext } from '../context/wallet';

export const Delegations = () => {
  const { delegations, unDelegateAllLoading, balanceLoading, accountLoading, account, clientsAreLoading } = useWalletContext();
  const [delegationNodeId, setDelegationNodeId] = useState<string>();
  const [amountToBeDelegated, setAmountToBeDelegated] = useState<string>();
  // const [log, setLog] = useState<React.ReactNode[]>([]);
  // const [delegations, setDelegations] = useState<any>();
  // const [delegationLoader, setDelegationLoader] = useState<boolean>(false);
  // const [withdrawLoading, setWithdrawLoading] = useState<boolean>(false);

  // Start Delegate
  // const doDelegate = async ({ mixId, amount }: { mixId: number; amount: number }) => {
  //   if (!signerClient) {
  //     return;
  //   }
  //   setDelegationLoader(true);
  //   try {
  //     const res = await signerClient.delegateToMixnode({ mixId }, 'auto', undefined, [
  //       { amount: `${amount}`, denom: 'unym' },
  //     ]);
  //     console.log('res', res);
  //     setLog((prev) => [
  //       ...prev,
  //       <div key={JSON.stringify(res, null, 2)}>
  //         <code style={{ marginRight: '2rem' }}>{new Date().toLocaleTimeString()}</code>
  //         <pre>{JSON.stringify(res, null, 2)}</pre>
  //       </div>,
  //     ]);
  //   } catch (error) {
  //     console.error(error);
  //   }
  //   setDelegationLoader(false);
  // };
  // End delegate

  // Start Withdraw Rewards
  // const doWithdrawRewards = async () => {
  //   const delegatorAddress = '';
  //   const validatorAdress = '';
  //   const memo = 'test sending tokens';
  //   setWithdrawLoading(true);
  //   try {
  //     const res = await signerCosmosWasmClient.withdrawRewards(delegatorAddress, validatorAdress, 'auto', memo);
  //     setLog((prev) => [
  //       ...prev,
  //       <div key={JSON.stringify(res, null, 2)}>
  //         <code style={{ marginRight: '2rem' }}>{new Date().toLocaleTimeString()}</code>
  //         <pre>{JSON.stringify(res, null, 2)}</pre>
  //       </div>,
  //     ]);
  //   } catch (error) {
  //     console.error(error);
  //   }
  //   setWithdrawLoading(false);
  // };
  // End Withdraw Rewards

  // useEffect(() => {
  //   if (signerClient && !delegations) {
  //     getDelegations();
  //   }
  // }, [signerClient, getDelegations, delegations]);

  useEffect(() => {
    console.log('delegations', delegations);
  },[delegations]);

  return (
    <Paper style={{ marginTop: '1rem', padding: '1rem' }}>
      <Box padding={3}>
        <Typography variant="h6">Delegations</Typography>
        <Box marginY={3}>
          <Box marginY={3} display="flex" flexDirection="column">
            <Typography marginBottom={3} variant="body1">
              Make a delegation
            </Typography>
            <TextField
              type="text"
              placeholder="Mixnode ID"
              onChange={(e) => setDelegationNodeId(e.target.value)}
              size="small"
            />
            <Box marginTop={3} display="flex" justifyContent="space-between">
              <TextField
                type="text"
                placeholder="Amount"
                onChange={(e) => setAmountToBeDelegated(e.target.value)}
                size="small"
              />
              {/* <Button
                variant="outlined"
                onClick={() =>
                  doDelegate({ mixId: parseInt(delegationNodeId, 10), amount: parseInt(amountToBeDelegated, 10) })
                }
                disabled={delegationLoader}
              >
                {delegationLoader ? 'Delegation in process...' : 'Delegate'}
              </Button> */}
            </Box>
          </Box>
        </Box>
        <Box marginTop={3}>
          <Typography variant="body1">Your delegations</Typography>
          <Box marginBottom={3} display="flex" flexDirection="column">
            {!delegations?.delegations?.length ? (
              <Typography>You do not have delegations</Typography>
            ) : (
              <Box>
                <Table size="small">
                  <TableHead>
                    <TableRow>
                      <TableCell>MixId</TableCell>
                      <TableCell>Owner</TableCell>
                      <TableCell>Amount</TableCell>
                      <TableCell>Cumulative Reward Ratio</TableCell>
                    </TableRow>
                  </TableHead>
                  <TableBody>
                    {delegations?.delegations.map((delegation: any) => (
                      <TableRow key={delegation.mix_id}>
                        <TableCell>{delegation.mix_id}</TableCell>
                        <TableCell>{delegation.owner}</TableCell>
                        <TableCell>{delegation.amount.amount}</TableCell>
                        <TableCell>{delegation.cumulative_reward_ratio}</TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </Box>
            )}
          </Box>
          {delegations && (
            <Box marginBottom={3}>
              <Button variant="outlined" onClick={() => undelegateAll()} disabled={unDelegateAllLoading}>
                {unDelegateAllLoading ? 'Undelegating...' : 'Undelegate All'}
              </Button>
            </Box>
          )}
          <Box>
            {/* <Button variant="outlined" onClick={() => doWithdrawRewards()} disabled={withdrawLoading}>
              {withdrawLoading ? 'Doing withdraw...' : 'Withdraw rewards'}
            </Button> */}
          </Box>
        </Box>
      </Box>
    </Paper>
  );
};
