import { invoke } from '@tauri-apps/api';
import { Account } from '../types';

export const createMnemonic = async (): Promise<string> => invoke('create_new_mnemonic');

export const createPassword = async ({ mnemonic, password }: { mnemonic: string; password: string }): Promise<void> => {
  await invoke('create_password', { mnemonic, password });
};

export const signInWithMnemonic = async (mnemonic: string): Promise<Account> => {
  const res: Account = await invoke('connect_with_mnemonic', { mnemonic });
  return res;
};

export const validateMnemonic = async (mnemonic: string): Promise<boolean> => {
  const res: boolean = await invoke('validate_mnemonic', { mnemonic });
  return res;
};

export const signInWithPassword = async (password: string): Promise<Account> => {
  const res: Account = await invoke('sign_in_with_password', { password });
  return res;
};

export const signOut = async (): Promise<void> => {
  await invoke('logout');
};

export const isPasswordCreated = async (): Promise<boolean> => {
  const res: boolean = await invoke('does_password_file_exist');
  return res;
};

export const addAccount = async ({
  mnemonic,
  password,
  accountName,
}: {
  mnemonic: string;
  password: string;
  accountName: string;
}): Promise<void> => {
  await invoke('add_account_for_password', { mnemonic, password, innerId: accountName });
};

export const listAccounts = async (password: string) => {
  const res: Account[] = await invoke('list_accounts_for_password', { password });
  return res;
};
