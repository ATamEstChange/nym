import { Card, CardHeader, Link, Typography } from '@mui/material'
import { urls } from '../context/index'

export const TokenTransfer = ({
  address,
  amount,
}: {
  address: string
  amount: string
}) => {
  return (
    <Card
      sx={{
        background: 'transparent',
        border: (theme) => `1px solid ${theme.palette.common.white}`,
        p: 2,
        overflow: 'auto',
      }}
    >
      <CardHeader
        title={
          <>
            <Typography component="span" variant="h5">
              Successfully transferred {amount} NYMT to
            </Typography>{' '}
            <Link
              target="_blank"
              rel="noopener"
              href={`${urls.blockExplorer}/account/${address}`}
              data-testid="success-sent-message"
              variant="h5"
            >
              {address}
            </Link>
          </>
        }
      />
    </Card>
  )
}
